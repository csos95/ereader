mod epub;
mod library;
mod scan;

use crate::epub::html_to_styled_string;
use async_std::task;
use cursive::traits::Scrollable;
use cursive::views::{Dialog, SelectView, TextView};
use cursive::{Cursive, CursiveExt};
use sqlx::SqlitePool;
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
    #[error("archive error {0}")]
    ArchiveError(epub::ArchiveError),
}

impl From<sqlx::Error> for DatabaseError {
    fn from(e: sqlx::Error) -> Self {
        DatabaseError::SqlxError(e)
    }
}

impl From<epub::ArchiveError> for DatabaseError {
    fn from(e: epub::ArchiveError) -> Self {
        DatabaseError::ArchiveError(e)
    }
}

#[async_std::main]
async fn main() {
    let pool = SqlitePool::connect("ereader.sqlite").await.unwrap();
    scan::scan(&pool, "epub").await.unwrap();

    let mut siv = Cursive::new();
    siv.set_user_data(pool);

    library(&mut siv).unwrap();

    siv.add_global_callback('q', |s| s.quit());
    siv.add_global_callback('l', |s| {
        if let Err(e) = library(s) {
            error(s, e);
        }
    });
    siv.run();
}

fn error(s: &mut Cursive, e: DatabaseError) {
    s.add_layer(
        Dialog::around(TextView::new(format!("{:?}", e)))
            .title("Error")
            .button("Close", |s| { s.pop_layer(); } )
    );
}

fn library(s: &mut Cursive) -> Result<(), DatabaseError> {
    let books = task::block_on(async {
        let pool = s.user_data().unwrap();
        library::get_books(pool).await
    })?;

    let mut view = SelectView::new();

    for book in books {
        view.add_item(book.title, book.id);
    }

    view.set_on_submit(|s, id| {
        if let Err(e) = chapter(s, *id, 0) {
            error(s, e);
        }
    });

    s.pop_layer();
    s.add_layer(Dialog::around(view.scrollable()).title("Library"));

    Ok(())
}

fn chapter(s: &mut Cursive, id: i64, index: usize) -> Result<(), DatabaseError> {
    let book = task::block_on(async {
        let pool = s.user_data().unwrap();
        library::get_book(pool, id).await
    })?;

    let html = epub::get_chapter_html(book.path, index)?;
    let styled_text = html_to_styled_string("body", &html[..])?;

    let mut dialog = Dialog::around(TextView::new(styled_text).scrollable());

    let index_p = index;
    let id_p = id;
    if index > 0 {
        dialog.add_button("Previous", move |s| {
            if let Err(e) = chapter(s, id_p, index_p - 1) {
                error(s, e);
            }
        });
    }

    if true {
        dialog.add_button("Next", move |s| {
            if let Err(e) = chapter(s, id, index + 1) {
                error(s, e);
            }
        });
    }

    s.pop_layer();
    s.add_layer(dialog);

    Ok(())
}








