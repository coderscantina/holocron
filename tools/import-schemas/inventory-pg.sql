-- Column inventory for PostgreSQL. Run with psql -At. Types come from pg_catalog.format_type.
select jsonb_pretty(jsonb_build_object('engine','postgresql','tables',coalesce(jsonb_agg(t order by t->>'name'),'[]'::jsonb)))
from (
  select jsonb_build_object(
    'name', c.relname,
    'columns', (select jsonb_agg(jsonb_build_object(
        'name', a.attname, 'position', a.attnum, 'type', format_type(a.atttypid, a.atttypmod),
        'nullable', not a.attnotnull, 'default', pg_get_expr(d.adbin, d.adrelid)) order by a.attnum)
      from pg_attribute a left join pg_attrdef d on d.adrelid=a.attrelid and d.adnum=a.attnum
      where a.attrelid=c.oid and a.attnum>0 and not a.attisdropped),
    'indexes', coalesce((select jsonb_agg(jsonb_build_object(
        'name', ic.relname, 'unique', i.indisunique, 'primary', i.indisprimary,
        'columns', (select jsonb_agg(a.attname order by k.ord) from unnest(i.indkey) with ordinality k(attnum,ord)
                    join pg_attribute a on a.attrelid=c.oid and a.attnum=k.attnum)) order by ic.relname)
      from pg_index i join pg_class ic on ic.oid=i.indexrelid where i.indrelid=c.oid), '[]'),
    'foreignKeys', coalesce((select jsonb_agg(jsonb_build_object(
        'name', con.conname,
        'columns', (select jsonb_agg(a.attname order by k.ord) from unnest(con.conkey) with ordinality k(attnum,ord)
                    join pg_attribute a on a.attrelid=con.conrelid and a.attnum=k.attnum),
        'refTable', rc.relname,
        'refColumns', (select jsonb_agg(a.attname order by k.ord) from unnest(con.confkey) with ordinality k(attnum,ord)
                    join pg_attribute a on a.attrelid=con.confrelid and a.attnum=k.attnum),
        'onDelete', con.confdeltype, 'onUpdate', con.confupdtype) order by con.conname)
      from pg_constraint con join pg_class rc on rc.oid=con.confrelid where con.conrelid=c.oid and con.contype='f'), '[]')
  ) t
  from pg_class c join pg_namespace n on n.oid=c.relnamespace
  where n.nspname='public' and c.relkind='r' and c.relname<>'_prisma_migrations'
) s;
