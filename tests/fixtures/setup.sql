-- Test-only objects, run by tests/database.rs after the example's schema.sql.

-- A small table that the fixture boards read.
drop table if exists earthquakes_daily;

create table earthquakes_daily (
    day           date          not null,
    region        text          not null,
    events        integer       not null,
    max_magnitude numeric(3, 1) not null,
    primary key (day, region)
);

-- Thirty days, four regions, values from a fixed formula so that every run
-- gives the same rows.
insert into earthquakes_daily (day, region, events, max_magnitude)
select
    d::date,
    r,
    1 + ((d::date - date '2024-01-01') * 7 + length(r) * 3) % 20,
    3.0 + (((d::date - date '2024-01-01') * 3 + length(r)) % 25) / 10.0
from generate_series(date '2024-01-01', date '2024-01-30', interval '1 day') as d,
     (values ('Andes'), ('Cascadia'), ('Hokkaido'), ('Rift Valley')) as regions (r);

drop table if exists scratch cascade;
create table scratch (n integer not null);
insert into scratch values (0);

create or replace function scratch_bump() returns integer
language sql volatile
as $$ update scratch set n = n + 1 returning n $$;

-- Throwaway roles for the test database only.
do $$
begin
    if not exists (select from pg_roles where rolname = 'enlightenviz_reader') then
        create role enlightenviz_reader login password 'reader-test-only';
    end if;
    if not exists (select from pg_roles where rolname = 'enlightenviz_load') then
        create role enlightenviz_load login password 'load-test-only';
    end if;
    if not exists (select from pg_roles where rolname = 'enlightenviz_disconnect') then
        create role enlightenviz_disconnect login password 'disconnect-test-only';
    end if;
    if not exists (select from pg_roles where rolname = 'enlightenviz_writer') then
        create role enlightenviz_writer login password 'writer-test-only';
    end if;
end
$$;

grant select on earthquakes_daily, earthquake_events to enlightenviz_reader, enlightenviz_load,
    enlightenviz_writer, enlightenviz_disconnect;
-- This role can write, so that only the read-only transaction stops a write.
grant select, update on scratch to enlightenviz_writer;
