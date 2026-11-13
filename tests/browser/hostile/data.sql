-- Hostile data for the browser test: text that would run or draw something if
-- the page ever treated it as markup. Nothing here comes from a real system.
-- Safe to run again: it drops and recreates its own table.
--
-- The last row has NULLs: an option with no value, which the filters must not
-- offer and must not send as the text "null".

drop table if exists hostile_events;

create table hostile_events (
    place text,
    kind  text,
    n     integer not null
);

insert into hostile_events (place, kind, n) values
    ('<img src=x onerror="window.pwned = 1">', '<i>alpha</i>', 5),
    ('<script>window.pwned = 1</script>',      '<u>beta</u>',  3),
    ('<b>bold</b> &amp; an entity',            '<i>alpha</i>', 8),
    (null,                                     null,           2);
