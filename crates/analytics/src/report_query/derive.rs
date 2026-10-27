//! Derived columns of a cut block (0022 Derived columns): ratio, difference and share over
//! measure columns, computed per row and for the totals.

use holocron_protocol::exploration::{Metric, Money, Reason};
use holocron_protocol::report_query::{
    Block, BlockBody, BlockResult, ColumnRole, ColumnType, Derived, Operand, ReportCell,
    TotalOperand,
};

use super::table::Layout;
use super::{Checked, Refusal, valid_name};
use crate::explore::{decimal, ratio4};

/// Checks names and operands: measure columns of this block, or totals of an earlier cut
/// without `for_each`; ratios are no operands, and money pairs only with money.
pub(super) fn check(
    path: &str,
    block: &Block,
    layout: &Layout,
    earlier: &[Block],
    checked: &[Checked],
) -> Result<(), Refusal> {
    for (j, derived) in block.derive.iter().enumerate() {
        let at = format!("{path}.derive.{j}");
        let name = match derived {
            Derived::Ratio(d) | Derived::Difference(d) => &d.name,
            Derived::Share(d) => &d.name,
        };
        if !valid_name(name) {
            return Err(Refusal::invalid(
                format!("{at}.name"),
                "derived names match ^[a-z][a-z0-9_]{0,31}$",
            ));
        }
        if layout.specs().iter().filter(|s| s.name == *name).count() > 1 {
            return Err(Refusal::invalid(
                format!("{at}.name"),
                "another column of the block has this name",
            ));
        }
        let kind = |operand: &Operand, at: String| match operand {
            Operand::Column(c) => column_kind(layout, &c.column, at),
            Operand::Total(t) => total_kind(earlier, checked, t, at),
        };
        match derived {
            Derived::Share(d) => {
                column_kind(layout, &d.a.column, format!("{at}.a"))?;
            }
            Derived::Ratio(d) | Derived::Difference(d) => {
                let a = kind(&d.a, format!("{at}.a"))?;
                let b = kind(&d.b, format!("{at}.b"))?;
                let money = |k: Option<ColumnType>| k == Some(ColumnType::Money);
                if money(a) != money(b) {
                    return Err(Refusal::invalid(
                        format!("{at}.b"),
                        "money pairs only with money",
                    ));
                }
                if matches!(derived, Derived::Difference(_)) && a != b {
                    return Err(Refusal::invalid(
                        format!("{at}.b"),
                        "a difference needs two columns of the same type",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// The planned type of a measure column; `None` when it depends on the data.
fn column_kind(layout: &Layout, column: &str, at: String) -> Result<Option<ColumnType>, Refusal> {
    let spec = layout
        .specs()
        .iter()
        .find(|s| s.name == column && s.role == ColumnRole::Measure)
        .ok_or_else(|| {
            Refusal::invalid(
                at.clone(),
                format!("the block has no measure column `{column}`"),
            )
        })?;
    if spec.kind == Some(ColumnType::Ratio) {
        return Err(Refusal::invalid(at, "a ratio column is no operand"));
    }
    Ok(spec.kind)
}

fn total_kind(
    earlier: &[Block],
    checked: &[Checked],
    total: &TotalOperand,
    at: String,
) -> Result<Option<ColumnType>, Refusal> {
    if !total.total {
        return Err(Refusal::invalid(
            at,
            "`total` is always true; use `{column}` for this block's rows",
        ));
    }
    let Some(j) = earlier.iter().position(|b| b.name == total.block) else {
        return Err(Refusal::invalid(
            at,
            format!("`{}` names no earlier block", total.block),
        ));
    };
    let source = &earlier[j];
    if source.for_each.is_some() || !matches!(source.body, BlockBody::Cut(_)) {
        return Err(Refusal::invalid(
            at,
            "totals exist for cut blocks without for_each only",
        ));
    }
    column_kind(&checked[j].layout, &total.column, at)
}

/// Appends the derived cells to one run's rows and totals. `rows` lack the `for_each` column;
/// `totals` are that run's.
pub(super) fn apply(
    block: &Block,
    layout: &Layout,
    done: &[BlockResult],
    rows: &mut [Vec<ReportCell>],
    totals: &mut Option<Vec<ReportCell>>,
) {
    let offset = usize::from(block.for_each.is_some());
    let index = |column: &str| layout.index(column).expect("checked in plan") - offset;
    for derived in &block.derive {
        let source = |operand: &Operand| match operand {
            Operand::Column(c) => Source::Column(index(&c.column)),
            Operand::Total(t) => Source::Fixed(total(done, t)),
        };
        let (op, a, b): (fn(&Metric, &Metric) -> Metric, _, _) = match derived {
            Derived::Ratio(d) => (ratio, source(&d.a), source(&d.b)),
            Derived::Difference(d) => (difference, source(&d.a), source(&d.b)),
            Derived::Share(d) => {
                let a = index(&d.a.column);
                let total = totals
                    .as_ref()
                    .map_or(unavailable(Reason::NoValues), |t| metric(&t[a]));
                (
                    ratio as fn(&Metric, &Metric) -> Metric,
                    Source::Column(a),
                    Source::Fixed(total),
                )
            }
        };
        for row in rows.iter_mut() {
            let cell = op(&a.get(row), &b.get(row));
            row.push(ReportCell::Metric(cell));
        }
        if let Some(totals) = totals {
            let b = match derived {
                Derived::Share(_) => a.get(totals),
                _ => b.get(totals),
            };
            let cell = op(&a.get(totals), &b);
            totals.push(ReportCell::Metric(cell));
        }
    }
}

enum Source {
    Column(usize),
    Fixed(Metric),
}

impl Source {
    fn get(&self, row: &[ReportCell]) -> Metric {
        match self {
            Source::Column(i) => metric(&row[*i]),
            Source::Fixed(m) => m.clone(),
        }
    }
}

fn metric(cell: &ReportCell) -> Metric {
    match cell {
        ReportCell::Metric(m) => m.clone(),
        ReportCell::Key(_) => unavailable(Reason::NoValues),
    }
}

fn total(done: &[BlockResult], operand: &TotalOperand) -> Metric {
    let block = done
        .iter()
        .find(|b| b.name == operand.block)
        .expect("checked in plan");
    let i = block
        .columns
        .iter()
        .position(|c| c.name == operand.column)
        .expect("checked in plan");
    block
        .totals
        .as_ref()
        .map_or(unavailable(Reason::NoValues), |t| metric(&t[i]))
}

fn unavailable(reason: Reason) -> Metric {
    Metric::Unavailable { reason }
}

/// An operand as an exact number: a scalar with its decimal scale, or amounts per currency.
enum Num {
    Scalar { v: i128, scale: u32, kind: Kind },
    Money(Vec<Money>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Count,
    Millis,
    Decimal,
}

fn num(metric: &Metric) -> Result<Num, Reason> {
    let scalar = |v: i128, scale, kind| Ok(Num::Scalar { v, scale, kind });
    match metric {
        Metric::Count { v } => scalar(i128::from(*v), 0, Kind::Count),
        Metric::Millis { v } => scalar(i128::from(*v), 0, Kind::Millis),
        Metric::Decimal { v } => {
            scaled(v).map_or(Err(Reason::NoValues), |v| scalar(v, 4, Kind::Decimal))
        }
        Metric::Money { v } => Ok(Num::Money(v.clone())),
        Metric::Unavailable { reason } => Err(*reason),
        Metric::Ratio { .. } => Err(Reason::NoValues),
    }
}

/// A decimal string with 4 places as an integer scaled by 10^4.
fn scaled(text: &str) -> Option<i128> {
    let (negative, digits) = text
        .strip_prefix('-')
        .map_or((false, text), |rest| (true, rest));
    let (whole, fraction) = digits.split_once('.')?;
    let v = whole.parse::<i128>().ok()? * 10_000 + fraction.parse::<i128>().ok()?;
    Some(if negative { -v } else { v })
}

/// The single currency of two money operands and their minor units; `Err` with
/// `mixed_currencies` when they carry more than one. No amount counts as 0.
fn same_currency(a: &[Money], b: &[Money]) -> Result<(Option<String>, i128, i128), Reason> {
    let mut codes: Vec<&str> = a.iter().chain(b).map(|m| m.currency.as_str()).collect();
    codes.sort_unstable();
    codes.dedup();
    if codes.len() > 1 {
        return Err(Reason::MixedCurrencies);
    }
    let units = |m: &[Money]| m.first().map_or(0, |m| m.minor_units);
    Ok((codes.first().map(|c| (*c).to_owned()), units(a), units(b)))
}

fn ratio(a: &Metric, b: &Metric) -> Metric {
    let parts = match (num(a), num(b)) {
        (Err(reason), _) | (_, Err(reason)) => return unavailable(reason),
        (
            Ok(Num::Scalar {
                v: x, scale: sx, ..
            }),
            Ok(Num::Scalar {
                v: y, scale: sy, ..
            }),
        ) => Ok((x * 10_i128.pow(sy), y * 10_i128.pow(sx))),
        (Ok(Num::Money(x)), Ok(Num::Money(y))) => same_currency(&x, &y).map(|(_, x, y)| (x, y)),
        _ => Err(Reason::MixedCurrencies),
    };
    match parts {
        Err(reason) => unavailable(reason),
        Ok((_, 0)) => unavailable(Reason::NoDenominator),
        Ok((n, d)) => Metric::Ratio {
            v: ratio4(n, d),
            numerator: n as i64,
            denominator: d as i64,
        },
    }
}

fn difference(a: &Metric, b: &Metric) -> Metric {
    match (num(a), num(b)) {
        (Err(reason), _) | (_, Err(reason)) => unavailable(reason),
        (Ok(Num::Scalar { v: x, kind: kx, .. }), Ok(Num::Scalar { v: y, kind: ky, .. }))
            if kx == ky =>
        {
            let v = x - y;
            match kx {
                Kind::Count => Metric::Count { v: v as i64 },
                Kind::Millis => Metric::Millis { v: v as i64 },
                Kind::Decimal => Metric::Decimal { v: decimal(v) },
            }
        }
        // Percentiles of CLS are decimals, of other metrics milliseconds.
        (Ok(Num::Scalar { .. }), Ok(Num::Scalar { .. })) => unavailable(Reason::MixedMetrics),
        (Ok(Num::Money(x)), Ok(Num::Money(y))) => match same_currency(&x, &y) {
            Err(reason) => unavailable(reason),
            Ok((code, x, y)) => Metric::Money {
                v: code
                    .and_then(|code| Money::new(&code, x - y))
                    .into_iter()
                    .collect(),
            },
        },
        _ => unavailable(Reason::MixedCurrencies),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn money(code: &str, minor: i128) -> Metric {
        Metric::Money {
            v: vec![Money::new(code, minor).unwrap()],
        }
    }

    #[test]
    fn ratios_round_half_to_even_and_refuse_zero_denominators() {
        let count = |v| Metric::Count { v };
        assert_eq!(
            ratio(&count(1), &count(3)),
            Metric::Ratio {
                v: 0.3333,
                numerator: 1,
                denominator: 3
            }
        );
        assert_eq!(
            ratio(&count(1), &count(0)),
            unavailable(Reason::NoDenominator)
        );
        let half = Metric::Decimal { v: "0.5000".into() };
        assert_eq!(
            ratio(&half, &count(2)),
            Metric::Ratio {
                v: 0.25,
                numerator: 5_000,
                denominator: 20_000
            }
        );
    }

    #[test]
    fn money_keeps_one_currency_per_cell() {
        assert_eq!(
            difference(&money("EUR", 5_000), &money("EUR", 1_250)),
            money("EUR", 3_750)
        );
        assert_eq!(
            difference(&money("EUR", 5_000), &money("USD", 1_250)),
            unavailable(Reason::MixedCurrencies)
        );
        assert_eq!(
            ratio(&money("EUR", 1_000), &Metric::Money { v: vec![] }),
            unavailable(Reason::NoDenominator)
        );
    }
}
