-- Invented data for the earthquakes example board. Nothing here comes from a
-- real system. Safe to run again: it drops and recreates its own table.

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

-- To serve the board, give the server a role that can only read this table:
--
--   create role enlightenviz_reader login password '<choose a password>';
--   grant select on earthquakes_daily to enlightenviz_reader;
