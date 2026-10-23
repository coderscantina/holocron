-- Column inventory for MySQL, from information_schema. Run with mysql -N -B -r.
select json_pretty(json_object('engine','mysql','tables',coalesce((select json_arrayagg(t) from (
  select json_object(
    'name', t.table_name,
    'columns', (select json_arrayagg(json_object('name',c.column_name,'type',c.column_type,
        'position',c.ordinal_position,'nullable',c.is_nullable='YES','default',c.column_default,'extra',nullif(c.extra,''))) from
        (select * from information_schema.columns where table_schema=database() and table_name=t.table_name order by ordinal_position) c),
    'indexes', coalesce((select json_arrayagg(json_object('name',x.index_name,'unique',x.non_unique=0,
        'primary',x.index_name='PRIMARY','columns',x.cols)) from
        (select index_name, min(non_unique) non_unique,
           cast(concat('[', group_concat(json_quote(column_name) order by seq_in_index), ']') as json) cols
         from information_schema.statistics where table_schema=database() and table_name=t.table_name
         group by index_name order by index_name) x), json_array()),
    'foreignKeys', coalesce((select json_arrayagg(json_object('name',f.constraint_name,'columns',f.cols,
        'refTable',f.ref_table,'refColumns',f.ref_cols,'onDelete',f.delete_rule,'onUpdate',f.update_rule)) from
        (select k.constraint_name,
           cast(concat('[', group_concat(json_quote(k.column_name) order by k.ordinal_position), ']') as json) cols,
           k.referenced_table_name ref_table,
           cast(concat('[', group_concat(json_quote(k.referenced_column_name) order by k.ordinal_position), ']') as json) ref_cols,
           r.delete_rule, r.update_rule
         from information_schema.key_column_usage k
         join information_schema.referential_constraints r
           on r.constraint_schema=k.constraint_schema and r.constraint_name=k.constraint_name
         where k.table_schema=database() and k.table_name=t.table_name and k.referenced_table_name is not null
         group by k.constraint_name, k.referenced_table_name, r.delete_rule, r.update_rule
         order by k.constraint_name) f), json_array())
  ) t
  from information_schema.tables t where t.table_schema=database() and t.table_type='BASE TABLE'
    and t.table_name<>'_prisma_migrations' order by t.table_name) q), json_array())));
