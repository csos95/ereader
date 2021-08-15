use crate::scan::SourceBook;
use sqlx::SqlitePool;
use sqlx::{query, query_as};

#[derive(Clone, Debug)]
pub struct Book {
    pub id: i64,
    pub identifier: String,
    pub language: String,
    pub title: String,
    pub creator: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub path: String,
}

pub async fn insert_book(pool: &SqlitePool, book: &SourceBook) -> Result<(), sqlx::Error> {
    query!("insert into books(identifier, language, title, creator, description, publisher, path) values (?, ?, ?, ?, ?, ?, ?)",
    book.identifier, book.language, book.title, book.creator, book.description, book.publisher, book.path)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_book_path(pool: &SqlitePool, book: &Book) -> Result<(), sqlx::Error> {
    query!("update books set path = ? where id = ?", book.path, book.id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_books(pool: &SqlitePool) -> Result<Vec<Book>, sqlx::Error> {
    Ok(query_as!(Book, "select * from books")
        .fetch_all(pool)
        .await?)
}

pub async fn get_book(pool: &SqlitePool, id: i64) -> Result<Book, sqlx::Error> {
    Ok(query_as!(Book, "select * from books where id = ?", id)
        .fetch_one(pool)
        .await?)
}
