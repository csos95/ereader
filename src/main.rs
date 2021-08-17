mod epub;
mod library;
mod scan;

use crate::epub::html_to_styled_string;
use async_std::task;
use cursive::traits::Scrollable;
use cursive::views::{Dialog, SelectView, TextView};
use cursive::{Cursive, CursiveExt};
use ::epub::doc::EpubDoc;
use sqlx::SqlitePool;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("sqlx error {0}")]
    SqlxError(sqlx::Error),
    #[error("unable to parse epub")]
    UnableToParseEpub,
    #[error("{0} is missing metadata tag {1}")]
    MissingMetadata(String, String),
    #[error("unable to get resource")]
    UnableToGetResource,
    #[error("invalid spine index: {0}")]
    InvalidSpineIndex(usize),
    #[error("anyhow error {0}")]
    AnyhowError(anyhow::Error),
    #[error("unable to parse html")]
    UnableToParseHTML,
    #[error("unable to find {0} in html")]
    UnableToFindSelector(String),
    #[error("io error {0}")]
    IOError(std::io::Error),
}

impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Error::SqlxError(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IOError(e)
    }
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Error::AnyhowError(e)
    }
}

struct UserData {
    pool: SqlitePool,
    epub: Option<EpubDoc<std::io::Cursor<Vec<u8>>>>,
}

#[async_std::main]
async fn main() {
    let pool = SqlitePool::connect("ereader.sqlite").await.unwrap();
    scan::scan(&pool, "epub").await.unwrap();

    let mut siv = Cursive::new();
    siv.set_user_data(UserData { pool, epub: None });

    library(&mut siv).unwrap();

    siv.add_global_callback('q', |s| s.quit());
    siv.add_global_callback('l', |s| {
        if let Err(e) = library(s) {
            error(s, e);
        }
    });
    siv.run();
}

fn error(s: &mut Cursive, e: Error) {
    s.add_layer(
        Dialog::around(TextView::new(format!("{:?}", e)))
            .title("Error")
            .button("Close", |s| {
                s.pop_layer();
            }),
    );
}

fn library(s: &mut Cursive) -> Result<(), Error> {
    let books = task::block_on(async {
        let user_data: &mut UserData = s.user_data().unwrap();
        library::get_books(&user_data.pool).await
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

fn chapter(s: &mut Cursive, id: i64, index: usize) -> Result<(), Error> {
    let book = task::block_on(async {
        let user_data: &mut UserData = s.user_data().unwrap();
        library::get_book(&user_data.pool, id).await
    })?;

    let html = epub::get_chapter_html(&book.path, index)?;
    let styled_text = html_to_styled_string("body", &html[..])?;

    let mut dialog = Dialog::around(TextView::new(styled_text).scrollable());

    let index_n = index;
    let id_n = id;
    if true {
        dialog.add_button("Next", move |s| {
            if let Err(e) = chapter(s, id_n, index_n + 1) {
                error(s, e);
            }
        });
    }

    if index > 0 {
        dialog.add_button("Previous", move |s| {
            if let Err(e) = chapter(s, id, index - 1) {
                error(s, e);
            }
        });
    }

    dialog.add_button("TOC", move |s| {
        if let Err(e) = toc(s, &book) {
            error(s, e);
        }
    });

    s.pop_layer();
    s.add_layer(dialog);

    Ok(())
}

fn toc(s: &mut Cursive, book: &library::Book) -> Result<(), Error> {
    let toc = epub::toc(&book.path)?;

    let mut view = SelectView::new();

    for (label, content) in toc {
        view.add_item(label, (book.id, content));
    }

    view.set_on_submit(|s, (id, _content)| {
        s.pop_layer();
        if let Err(e) = chapter(s, *id, 0) {
            error(s, e);
        }
    });

    s.add_layer(
        Dialog::around(view.scrollable())
            .title("Table of Contents")
            .button("Close", |s| {
                s.pop_layer();
            }),
    );

    Ok(())
}
