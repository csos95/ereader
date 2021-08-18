use crate::scan::{SourceBook, SourceChapter};
use crate::Error;
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
    pub hash: String,
}

pub async fn insert_book(pool: &SqlitePool, book: &SourceBook) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    query!("insert into books(identifier, language, title, creator, description, publisher, hash) values (?, ?, ?, ?, ?, ?, ?)",
    book.identifier, book.language, book.title, book.creator, book.description, book.publisher, book.hash)
        .execute(&mut tx)
        .await?;

    let row = query!("select last_insert_rowid() as id")
        .fetch_one(&mut tx)
        .await?;
    println!("{:?}", row);
    let book_id: i64 = row.id.into();
    for chapter in &book.chapters {
        insert_chapter(&mut tx, book_id, chapter).await?;
    }
    tx.commit().await?;

    Ok(())
}

pub async fn insert_chapter(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, book_id: i64, chapter: &SourceChapter) -> Result<(), sqlx::Error> {
    query!("insert into chapters(book_id, `index`, content) values (?, ?, ?)", book_id, chapter.index, chapter.content)
        .execute(tx)
        .await?;
    Ok(())
}

// pub async fn update_book_path(pool: &SqlitePool, book: &Book) -> Result<(), Error> {
//     query!("update books set path = ? where id = ?", book.path, book.id)
//         .execute(pool)
//         .await?;
//     Ok(())
// }

pub async fn get_books(pool: &SqlitePool) -> Result<Vec<Book>, Error> {
    Ok(query_as!(Book, "select * from books")
        .fetch_all(pool)
        .await?)
}

pub async fn get_book(pool: &SqlitePool, id: i64) -> Result<Book, Error> {
    Ok(query_as!(Book, "select * from books where id = ?", id)
        .fetch_one(pool)
        .await?)
}
