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

-- Invented events for the earthquakes-vega and earthquakes-echarts boards:
-- 360 events in January 2024 along six real plate boundaries (the Andes and
-- Cascadia and Tonga trenches, the Japan-Kuril trench, the East African rift
-- and the Aegean). Each value comes from the md5 of the event number, so
-- every run gives the same rows on any Postgres version. The magnitudes are
-- skewed towards small ones, as real catalogues are.

drop table if exists earthquake_events;

create table earthquake_events (
    event_id  integer       primary key,
    day       date          not null,
    region    text          not null,
    magnitude numeric(3, 1) not null,
    depth_km  integer       not null,
    latitude  numeric(5, 2) not null,
    longitude numeric(6, 2) not null
);

with boundaries (region, lons, lats, shallow, deep) as (values
    ('Aegean',      array[23, 26, 28],                  array[35, 37, 38.5],             5,  90),
    ('Andes',       array[-73.5, -71.5, -70.5, -71, -76, -79], array[-37, -30, -23, -17, -10, -3], 15, 220),
    ('Cascadia',    array[-126, -126.5, -127],          array[41, 45, 49],              10,  60),
    ('Hokkaido',    array[142, 144, 145, 148],          array[36, 40, 43, 45],          10, 120),
    ('Rift Valley', array[36, 37.5, 36, 34.5, 34],      array[12, 6, 0, -6, -10],        5,  30),
    ('Tonga',       array[-174, -175.5, -176],          array[-15, -21, -25],           10, 550)
), numbered as (
    select row_number() over (order by region) - 1 as k, * from boundaries
), draws as (
    select
        n,
        (('x' || substr(md5(n::text || 'a'), 1, 8))::bit(32)::bigint) / 4294967296.0 as day_u,
        (('x' || substr(md5(n::text || 'b'), 1, 8))::bit(32)::bigint) / 4294967296.0 as region_u,
        (('x' || substr(md5(n::text || 'c'), 1, 8))::bit(32)::bigint) / 4294967296.0 as along_u,
        (('x' || substr(md5(n::text || 'd'), 1, 8))::bit(32)::bigint) / 4294967296.0 as lon_u,
        (('x' || substr(md5(n::text || 'e'), 1, 8))::bit(32)::bigint) / 4294967296.0 as lat_u,
        (('x' || substr(md5(n::text || 'f'), 1, 8))::bit(32)::bigint) / 4294967296.0 as magnitude_u,
        (('x' || substr(md5(n::text || 'g'), 1, 8))::bit(32)::bigint) / 4294967296.0 as depth_u
    from generate_series(1, 360) as n
), placed as (
    select
        d.*, b.region, b.lons, b.lats, b.shallow, b.deep,
        d.along_u * (cardinality(b.lons) - 1) as position
    from draws d
    join numbered b on b.k = floor(power(d.region_u, 1.3) * 6)
)
insert into earthquake_events (event_id, day, region, magnitude, depth_km, latitude, longitude)
select
    n,
    date '2024-01-01' + floor(day_u * 30)::int,
    region,
    round((2.6 + 4.6 * power(magnitude_u, 4.5))::numeric, 1),
    (shallow + (deep - shallow) * power(depth_u, 2))::int,
    round((lats[floor(position)::int + 1]
           + (lats[floor(position)::int + 2] - lats[floor(position)::int + 1]) * (position - floor(position))
           + (lat_u - 0.5) * 1.6)::numeric, 2),
    round((lons[floor(position)::int + 1]
           + (lons[floor(position)::int + 2] - lons[floor(position)::int + 1]) * (position - floor(position))
           + (lon_u - 0.5) * 1.6)::numeric, 2)
from placed;

-- To serve the board, give the server a role that can only read this table:
--
--   create role enlightenviz_reader login password '<choose a password>';
--   grant select on earthquakes_daily, earthquake_events to enlightenviz_reader;
