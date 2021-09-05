use crate::Error;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use sqlx::{query, query_as};
use uuid::adapter::Hyphenated;

#[derive(Clone, Debug)]
pub struct Book {
    pub id: Hyphenated,
    pub identifier: String,
    pub language: String,
    pub title: String,
    pub creator: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub hash: String,
}

#[derive(Clone, Debug)]
pub struct Chapter {
    pub id: Hyphenated,
    pub book_id: Hyphenated,
    pub index: i64,
    pub content: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Toc {
    pub id: i64,
    pub book_id: Hyphenated,
    pub index: i64,
    pub chapter_id: Hyphenated,
    pub title: String,
}

#[derive(Clone, Debug)]
pub struct Bookmark {
    pub id: i64,
    pub book_id: Hyphenated,
    pub chapter_id: Hyphenated,
    pub progress: f32,
    pub created: DateTime<Utc>,
}

pub async fn insert_bookmark(pool: &SqlitePool, bookmark: &Bookmark) -> Result<(), Error> {
    query!("insert or replace into bookmarks(book_id, chapter_id, progress, created) values (?, ?, ?, ?)",
    bookmark.book_id, bookmark.chapter_id, bookmark.progress, bookmark.created)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn insert_book(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    book: &Book,
) -> Result<(), Error> {
    query!("insert into books(id, identifier, language, title, creator, description, publisher, hash) values (?, ?, ?, ?, ?, ?, ?, ?)",
    book.id, book.identifier, book.language, book.title, book.creator, book.description, book.publisher, book.hash)
        .execute(tx)
        .await?;
    Ok(())
}

pub async fn insert_chapter(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    chapter: &Chapter,
) -> Result<(), Error> {
    query!(
        "insert into chapters(id, book_id, `index`, content) values (?, ?, ?, ?)",
        chapter.id,
        chapter.book_id,
        chapter.index,
        chapter.content
    )
    .execute(tx)
    .await?;
    Ok(())
}

pub async fn insert_toc(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    toc: &Toc,
) -> Result<(), Error> {
    query!(
        "insert into table_of_contents(book_id, `index`, chapter_id, title) values (?, ?, ?, ?)",
        toc.book_id,
        toc.index,
        toc.chapter_id,
        toc.title
    )
    .execute(tx)
    .await?;
    Ok(())
}

pub async fn get_books(pool: &SqlitePool) -> Result<Vec<Book>, Error> {
    Ok(query_as!(Book, r#"select id as "id: Hyphenated", identifier, language, title, creator, description, publisher, hash from books order by title"#)
        .fetch_all(pool)
        .await?)
}

pub async fn get_book(pool: &SqlitePool, id: Hyphenated) -> Result<Book, Error> {
    Ok(query_as!(Book, r#"select id as "id: Hyphenated", identifier, language, title, creator, description, publisher, hash from books where id = ?"#, id)
        .fetch_one(pool)
        .await?)
}

pub async fn get_chapter(
    pool: &SqlitePool,
    book_id: Hyphenated,
    index: i64,
) -> Result<Chapter, Error> {
    Ok(query_as!(
        Chapter,
        r#"select id as "id: Hyphenated", book_id as "book_id: Hyphenated", `index`, content from chapters where book_id = ? and `index` = ?"#,
        book_id,
        index
    )
    .fetch_one(pool)
    .await?)
}

pub async fn get_chapter_by_id(pool: &SqlitePool, id: Hyphenated) -> Result<Chapter, Error> {
    Ok(
        query_as!(Chapter, r#"select id as "id: Hyphenated", book_id as "book_id: Hyphenated", `index`, content from chapters where id = ?"#, id)
            .fetch_one(pool)
            .await?,
    )
}

pub async fn get_num_chapters(pool: &SqlitePool, id: Hyphenated) -> Result<i32, Error> {
    Ok(
        sqlx::query_scalar!(r#"select count(*) from chapters where book_id = ?"#, id)
        .fetch_one(pool)
        .await?,
    )
}

pub async fn get_toc(pool: &SqlitePool, book_id: Hyphenated) -> Result<Vec<Toc>, Error> {
    Ok(query_as!(
        Toc,
        r#"select id, book_id as "book_id: Hyphenated", `index`, chapter_id as "chapter_id: Hyphenated", title from table_of_contents where book_id = ? order by `index`"#,
        book_id,
    )
    .fetch_all(pool)
    .await?)
}

pub async fn get_bookmarks(pool: &SqlitePool) -> Result<Vec<Bookmark>, Error> {
    Ok(query_as!(Bookmark, r#"select id, book_id as "book_id: Hyphenated", chapter_id as "chapter_id: Hyphenated", progress, created as "created: DateTime<Utc>" from bookmarks order by created desc"#)
       .fetch_all(pool)
       .await?)
}

pub async fn delete_bookmark(pool: &SqlitePool, chapter_id: i64) -> Result<(), Error> {
    query!("delete from bookmarks where chapter_id = ?", chapter_id)
        .execute(pool)
        .await?;
    Ok(())
}
