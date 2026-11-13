-- Test-only objects, run by tests/database.rs after the example's schema.sql.

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
