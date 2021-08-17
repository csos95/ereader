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

create table chapters (
    book_id integer not null,
    `index` integer not null,
    content text not null,
    primary key(book_id, `index`),
    foreign key (book_id) references books(id)
);

create table table_of_contents (
    book_id integer not null,
    `index` integer not null,
    chapter_id integer not null,
    title text not null,
    primary key(book_id, `index`),
    foreign key (book_id) references books(id),
    foreign key (chapter_id) references chapters(id)
);
