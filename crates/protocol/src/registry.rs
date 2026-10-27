//! The operation registry: names, access levels and wire types of every command and query.
//!
//! Each domain declares its operations with [`domain!`](crate::domain) in its own file. The macro
//! generates a handler trait with one method per operation, a dispatcher that parses arguments
//! into the declared type, and the [`OperationInfo`] list the exporter reads. The server
//! implements the traits, so a missing handler or a changed argument type is a compile error.

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Serialize;
use serde::de::DeserializeOwned;
use ts_rs::TS;

use crate::envelope::Topic;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `POST /api/commands/<name>`
    Command,
    /// `GET /api/queries/<name>`, or `POST` with the arguments as a JSON body.
    Query,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Callable without a session.
    Anonymous,
    /// Needs a signed-in person, or an API key or share that reaches the operation.
    Person,
}

/// When a command needs a password check within the last 10 minutes (decision 0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reauth {
    Never,
    /// Always. The dispatcher refuses to commit when the handler did not check.
    Always,
    /// The handler decides from the arguments, as `members.updateRole` does for admin and owner.
    Handler,
}

/// What a share must grant for its viewer to reach an operation. Anything else is `not_found`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareReach {
    None,
    Read,
    /// Needs the share's export action.
    Export,
    /// Needs the share's recordings grant.
    Recordings,
}

/// Who besides a full person session may call an operation, and what extra checks it needs.
/// Declared next to the operation in `domain!`, enforced by the server dispatcher. The defaults
/// are closed: no API key, no share viewer, no enrollment-only session, no reauthentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub reauth: Reauth,
    /// Whether an API key may hold the operation in its grants.
    pub key_grantable: bool,
    pub share: ShareReach,
    /// Whether an enrollment-only session may call it (decision 0007).
    pub enrollment: bool,
    /// The result discloses a one-time secret in its `secret` field. Stored answers keep
    /// `secret: null` and `secret_withheld: true`, so a replay never discloses it again.
    pub secret: bool,
}

impl Policy {
    pub const CLOSED: Self = Self {
        reauth: Reauth::Never,
        key_grantable: false,
        share: ShareReach::None,
        enrollment: false,
        secret: false,
    };

    pub const fn reauth(self) -> Self {
        Self {
            reauth: Reauth::Always,
            ..self
        }
    }

    pub const fn reauth_by_handler(self) -> Self {
        Self {
            reauth: Reauth::Handler,
            ..self
        }
    }

    pub const fn key(self) -> Self {
        Self {
            key_grantable: true,
            ..self
        }
    }

    pub const fn share_read(self) -> Self {
        Self {
            share: ShareReach::Read,
            ..self
        }
    }

    pub const fn share_export(self) -> Self {
        Self {
            share: ShareReach::Export,
            ..self
        }
    }

    pub const fn share_recordings(self) -> Self {
        Self {
            share: ShareReach::Recordings,
            ..self
        }
    }

    pub const fn enrollment(self) -> Self {
        Self {
            enrollment: true,
            ..self
        }
    }

    pub const fn secret(self) -> Self {
        Self {
            secret: true,
            ..self
        }
    }
}

/// How the exporter reaches one wire type.
#[derive(Clone, Copy)]
pub struct TypeInfo {
    pub ts_name: fn(&ts_rs::Config) -> String,
    pub export: fn(&ts_rs::Config) -> Result<(), ts_rs::ExportError>,
    pub schema: fn(&mut SchemaGenerator) -> Schema,
}

impl TypeInfo {
    pub const fn of<T: TS + JsonSchema + 'static>() -> Self {
        Self {
            ts_name: <T as TS>::name,
            export: export_all::<T>,
            schema: subschema::<T>,
        }
    }
}

fn export_all<T: TS + 'static>(cfg: &ts_rs::Config) -> Result<(), ts_rs::ExportError> {
    T::export_all(cfg)
}

fn subschema<T: JsonSchema>(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<T>()
}

#[derive(Clone, Copy)]
pub struct OperationInfo {
    pub name: &'static str,
    pub kind: Kind,
    pub access: Access,
    pub policy: Policy,
    pub summary: &'static str,
    pub args: TypeInfo,
    pub result: TypeInfo,
}

/// Context and error types shared by every domain's handler trait.
pub trait Handler: Sync {
    /// Owns the write transaction and the caller for one command.
    type CommandCtx: Send;
    /// Owns the read transaction and the caller for one query.
    type QueryCtx: Send;
    type Error: Send;
}

/// A query result together with the topic its revision and cursor come from.
pub struct Snapshot<T> {
    pub topic: Topic,
    pub data: T,
}

/// Where arguments come from: a JSON body for commands, a query string or a JSON body for queries.
pub trait ArgsSource: Sync {
    fn parse<T: DeserializeOwned>(&self) -> Result<T, String>;
}

pub struct JsonArgs(pub serde_json::Value);

impl ArgsSource for JsonArgs {
    /// Errors name the path they occurred at, as `blocks.2.body: unknown field ...`.
    fn parse<T: DeserializeOwned>(&self) -> Result<T, String> {
        serde_path_to_error::deserialize(&self.0).map_err(|error| {
            let path = error
                .path()
                .iter()
                .map(|segment| match segment {
                    serde_path_to_error::Segment::Seq { index } => index.to_string(),
                    serde_path_to_error::Segment::Map { key } => key.clone(),
                    serde_path_to_error::Segment::Enum { variant } => variant.clone(),
                    serde_path_to_error::Segment::Unknown => "?".to_owned(),
                })
                .collect::<Vec<_>>()
                .join(".");
            match path.as_str() {
                "" => error.into_inner().to_string(),
                _ => format!("{path}: {}", error.into_inner()),
            }
        })
    }
}

/// Repeated keys become arrays and absent keys stay absent.
pub struct FormArgs<'a>(pub &'a str);

impl ArgsSource for FormArgs<'_> {
    fn parse<T: DeserializeOwned>(&self) -> Result<T, String> {
        serde_html_form::from_str(self.0).map_err(|error| error.to_string())
    }
}

#[derive(Debug)]
pub enum DispatchError<E> {
    /// No operation of this kind has the name.
    Unknown,
    /// The arguments do not parse into the declared type.
    Args(String),
    Handler(E),
}

/// Serializes a result that must be a JSON object, so envelope fields can be merged into it.
pub fn result_object<T: Serialize>(result: &T) -> serde_json::Map<String, serde_json::Value> {
    match serde_json::to_value(result) {
        Ok(serde_json::Value::Object(map)) => map,
        Ok(_) => panic!("operation results are structs"),
        Err(error) => panic!("operation results serialize: {error}"),
    }
}

/// Declares one domain's operations. See the module docs.
///
/// After the access level come optional [`Policy`] flags: `key`, `share_read`, `share_export`,
/// `share_recordings`, `enrollment`, `reauth`, `reauth_by_handler` and `secret`.
///
/// ```ignore
/// domain! {
///     pub trait SitesHandlers;
///     commands {
///         /// Renames a site.
///         "sites.update" => sites_update (Person, key): SitesUpdateArgs => SitesUpdateResult;
///     }
///     queries {}
/// }
/// ```
#[macro_export]
macro_rules! domain {
    (
        $(#[$meta:meta])*
        pub trait $trait:ident;
        commands {
            $( $(#[doc = $cdoc:literal])* $cname:literal => $cfn:ident ($caccess:ident $(, $cflag:ident)*): $cargs:ty => $cres:ty; )*
        }
        queries {
            $( $(#[doc = $qdoc:literal])* $qname:literal => $qfn:ident ($qaccess:ident $(, $qflag:ident)*): $qargs:ty => $qres:ty; )*
        }
    ) => {
        $(#[$meta])*
        pub trait $trait: $crate::registry::Handler {
            $(
                $(#[doc = $cdoc])*
                fn $cfn(
                    &self,
                    ctx: &mut Self::CommandCtx,
                    args: $cargs,
                ) -> impl ::core::future::Future<Output = Result<$cres, Self::Error>> + Send;
            )*
            $(
                $(#[doc = $qdoc])*
                fn $qfn(
                    &self,
                    ctx: &mut Self::QueryCtx,
                    args: $qargs,
                ) -> impl ::core::future::Future<
                    Output = Result<$crate::registry::Snapshot<$qres>, Self::Error>,
                > + Send;
            )*
        }

        pub const OPERATIONS: &[$crate::registry::OperationInfo] = &[
            $(
                $crate::registry::OperationInfo {
                    name: $cname,
                    kind: $crate::registry::Kind::Command,
                    access: $crate::registry::Access::$caccess,
                    policy: $crate::registry::Policy::CLOSED $(.$cflag())*,
                    summary: concat!($($cdoc, "\n",)* ""),
                    args: $crate::registry::TypeInfo::of::<$cargs>(),
                    result: $crate::registry::TypeInfo::of::<$cres>(),
                },
            )*
            $(
                $crate::registry::OperationInfo {
                    name: $qname,
                    kind: $crate::registry::Kind::Query,
                    access: $crate::registry::Access::$qaccess,
                    policy: $crate::registry::Policy::CLOSED $(.$qflag())*,
                    summary: concat!($($qdoc, "\n",)* ""),
                    args: $crate::registry::TypeInfo::of::<$qargs>(),
                    result: $crate::registry::TypeInfo::of::<$qres>(),
                },
            )*
        ];

        /// Runs the named command if this domain declares it.
        #[allow(unused_variables)]
        pub async fn dispatch_command<H: $trait, S: $crate::registry::ArgsSource>(
            handler: &H,
            ctx: &mut H::CommandCtx,
            name: &str,
            args: &S,
        ) -> Result<
            ::serde_json::Map<String, ::serde_json::Value>,
            $crate::registry::DispatchError<H::Error>,
        > {
            match name {
                $(
                    $cname => {
                        let args: $cargs = args.parse().map_err($crate::registry::DispatchError::Args)?;
                        let result = handler
                            .$cfn(ctx, args)
                            .await
                            .map_err($crate::registry::DispatchError::Handler)?;
                        Ok($crate::registry::result_object(&result))
                    }
                )*
                _ => Err($crate::registry::DispatchError::Unknown),
            }
        }

        /// Runs the named query if this domain declares it.
        #[allow(unused_variables)]
        pub async fn dispatch_query<H: $trait, S: $crate::registry::ArgsSource>(
            handler: &H,
            ctx: &mut H::QueryCtx,
            name: &str,
            args: &S,
        ) -> Result<
            (::serde_json::Map<String, ::serde_json::Value>, $crate::envelope::Topic),
            $crate::registry::DispatchError<H::Error>,
        > {
            match name {
                $(
                    $qname => {
                        let args: $qargs = args.parse().map_err($crate::registry::DispatchError::Args)?;
                        let snapshot = handler
                            .$qfn(ctx, args)
                            .await
                            .map_err($crate::registry::DispatchError::Handler)?;
                        Ok(($crate::registry::result_object(&snapshot.data), snapshot.topic))
                    }
                )*
                _ => Err($crate::registry::DispatchError::Unknown),
            }
        }
    };
}

/// Merges the domain lists into one registry: a combined handler trait, lookup and dispatch.
macro_rules! domains {
    ($( $module:ident :: $trait:ident ),* $(,)?) => {
        /// Everything the server must implement.
        pub trait Handlers: $( $module::$trait + )* {}
        impl<T: $( $module::$trait + )*> Handlers for T {}

        /// Every operation of every domain, in domain order.
        pub fn all() -> impl Iterator<Item = &'static $crate::registry::OperationInfo> {
            [$( $module::OPERATIONS ),*].into_iter().flatten()
        }

        pub fn find(kind: $crate::registry::Kind, name: &str) -> Option<&'static $crate::registry::OperationInfo> {
            all().find(|op| op.kind == kind && op.name == name)
        }

        pub async fn dispatch_command<H: Handlers, S: $crate::registry::ArgsSource>(
            handler: &H,
            ctx: &mut H::CommandCtx,
            name: &str,
            args: &S,
        ) -> Result<
            ::serde_json::Map<String, ::serde_json::Value>,
            $crate::registry::DispatchError<H::Error>,
        > {
            $(
                match $module::dispatch_command(handler, ctx, name, args).await {
                    Err($crate::registry::DispatchError::Unknown) => {}
                    answer => return answer,
                }
            )*
            Err($crate::registry::DispatchError::Unknown)
        }

        pub async fn dispatch_query<H: Handlers, S: $crate::registry::ArgsSource>(
            handler: &H,
            ctx: &mut H::QueryCtx,
            name: &str,
            args: &S,
        ) -> Result<
            (::serde_json::Map<String, ::serde_json::Value>, $crate::envelope::Topic),
            $crate::registry::DispatchError<H::Error>,
        > {
            $(
                match $module::dispatch_query(handler, ctx, name, args).await {
                    Err($crate::registry::DispatchError::Unknown) => {}
                    answer => return answer,
                }
            )*
            Err($crate::registry::DispatchError::Unknown)
        }
    };
}

pub(crate) use domains;
