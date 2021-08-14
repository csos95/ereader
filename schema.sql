-- noinspection SqlNoDataSourceInspectionForFile

create table books (
    id integer not null primary key autoincrement,
    identifier text not null,
    language text not null,
    title text not null,
    creator text,
    description text,
    publisher text,
    path text not null
);

create index book_titles_idx on books(title);
create index book_creators_idx on books(creator);
create index book_publishers_idx on books(publisher);
