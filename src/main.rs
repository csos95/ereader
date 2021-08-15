mod epub;
mod library;
mod scan;

use crate::epub::html_to_styled_string;
use async_std::task;
use cursive::traits::Scrollable;
use cursive::views::{Dialog, SelectView, TextView};
use cursive::{Cursive, CursiveExt};
use once_cell::sync::OnceCell;
use sqlx::SqlitePool;
use std::sync::Mutex;
use std::sync::MutexGuard;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("database connection error: {0}")]
    ConnectionError(String),
    #[error("unable to set database connection")]
    UnableToSetConnection,
    #[error("unable to get database connection")]
    UnableToGetConnection,
    #[error("database connection is not set")]
    ConnectionNotSet,
    #[error("query error: {0}")]
    QueryError(String),
    #[error("misc error: {0}")]
    MiscError(String),
    #[error("diesel migration error: {0}")]
    MigrationError(String),
    #[error("sqlx error {0}")]
    SqlxError(sqlx::Error),
}

impl From<sqlx::Error> for DatabaseError {
    fn from(e: sqlx::Error) -> Self {
        DatabaseError::SqlxError(e)
    }
}

static POOL: OnceCell<Mutex<SqlitePool>> = OnceCell::new();

pub async fn init<S: std::convert::AsRef<str>>(path: S) -> Result<(), DatabaseError> {
    // connect to the database
    let pool = SqlitePool::connect(path.as_ref()).await?;

    // set the database connection
    POOL.set(Mutex::new(pool))
        .map_err(|_| DatabaseError::UnableToSetConnection)
}

async fn get_pool() -> Result<MutexGuard<'static, SqlitePool>, DatabaseError> {
    match POOL.get() {
        Some(mutex) => mutex
            .lock()
            .map_err(|_| DatabaseError::UnableToGetConnection),
        None => Err(DatabaseError::ConnectionNotSet),
    }
}

#[async_std::main]
async fn main() {
    init("ereader.sqlite").await.unwrap();

    //scan::scan(&pool, "epub").await.unwrap();

    let books = {
        let pool = get_pool().await.unwrap();
        library::get_books(&pool).await.unwrap()
    };

    let mut siv = Cursive::new();

    let mut view = SelectView::new().h_align(cursive::align::HAlign::Left);

    for book in &books {
        view.add_item(book.title.clone(), book.id);
    }

    view.set_on_submit(|s, id| {
        // s.pop_layer();
        let book = task::block_on(async {
            let pool = get_pool().await?;
            Ok::<library::Book, DatabaseError>(library::get_book(&pool, *id).await?)
        })
        .unwrap();
        let chapter_text = epub::get_chapter_html(book.path, 4).unwrap();
        let styled_text = html_to_styled_string("body", &chapter_text[..]).unwrap();
        s.add_layer(Dialog::around(TextView::new(styled_text).scrollable()));
    });

    siv.add_layer(Dialog::around(view.scrollable()).title("Library"));
    siv.add_global_callback('q', |s| s.quit());
    siv.run();
}
