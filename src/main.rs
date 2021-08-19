mod epub;
mod library;
mod scan;

use crate::epub::html_to_styled_string;
// use ::epub::doc::EpubDoc;
use async_std::task;
use cursive::traits::Scrollable;
use cursive::view::Resizable;
use cursive::views::{Dialog, SelectView, TextView};
use cursive::{Cursive, CursiveExt};
use library::{Book, Chapter, Toc};
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

struct Model {
    pool: SqlitePool,
    page: Page,
}

// struct Chapter {
//     epub: EpubDoc<std::io::Cursor<Vec<u8>>>,
//     path: String,
//     index: usize,
// }
//
// struct TableOfContents {
//     chapter: Chapter,
//     toc: Vec<(String, usize)>,
// }

enum Page {
    Library(Vec<Book>),
    Chapter(Chapter),
    TableOfContents(Vec<Toc>),
}

enum Msg {
    GoLibrary,
    GoChapterIndex(i64, i64),
    GoChapterId(i64),
    NextChapter,
    PrevChapter,
    GoTOC,
}

async fn init() -> Result<Model, Error> {
    let pool = SqlitePool::connect("ereader.sqlite").await?;
    scan::scan(&pool, "epub").await?;

    let books = library::get_books(&pool).await?;

    Ok(Model {
        pool,
        page: Page::Library(books),
    })
}

fn update_view(s: &mut Cursive, msg: Msg) {
    let mut model: Model = s.take_user_data().unwrap();

    model = update(msg, model).unwrap();
    s.pop_layer();
    view(s, &mut model).unwrap();

    s.set_user_data(model);
}

fn update(msg: Msg, mut model: Model) -> Result<Model, Error> {
    let pool = &model.pool;
    model.page = match (msg, model.page) {
        (Msg::GoLibrary, _) => {
            let books = task::block_on(async { library::get_books(pool).await })?;
            Page::Library(books)
        }
        (Msg::GoChapterIndex(book_id, index), _) => {
            let chapter =
                task::block_on(async { library::get_chapter(pool, book_id, index).await })?;
            Page::Chapter(chapter)
        }
        (Msg::NextChapter, Page::Chapter(chapter)) => {
            let chapter = task::block_on(async {
                library::get_chapter(pool, chapter.book_id, chapter.index + 1).await
            })?;
            Page::Chapter(chapter)
        }
        (Msg::PrevChapter, Page::Chapter(chapter)) => {
            let chapter = task::block_on(async {
                library::get_chapter(pool, chapter.book_id, chapter.index - 1).await
            })?;
            Page::Chapter(chapter)
        }
        (Msg::GoTOC, Page::Chapter(chapter)) => {
            let toc = task::block_on(async { library::get_toc(pool, chapter.book_id).await })?;
            Page::TableOfContents(toc)
        }
        (Msg::GoChapterId(id), _) => {
            let chapter = task::block_on(async { library::get_chapter_by_id(pool, id).await })?;
            Page::Chapter(chapter)
        }
        (_msg, page) => page,
    };

    Ok(model)
}

fn view(s: &mut Cursive, model: &mut Model) -> Result<(), Error> {
    match &mut model.page {
        Page::Chapter(chapter) => view_chapter(s, chapter)?,
        Page::Library(books) => view_library(s, books)?,
        Page::TableOfContents(toc) => view_toc(s, toc)?,
    }

    Ok(())
}

#[async_std::main]
async fn main() {
    let mut siv = Cursive::new();

    let mut model = init().await.unwrap();
    view(&mut siv, &mut model).unwrap();
    siv.set_user_data(model);

    siv.add_global_callback('q', |s| s.quit());
    siv.add_global_callback('l', |s| {
        s.cb_sink()
            .send(Box::new(move |s| update_view(s, Msg::GoLibrary)))
            .unwrap();
    });
    siv.run();
}

// fn error(s: &mut Cursive, e: Error) {
//     s.add_layer(
//         Dialog::around(TextView::new(format!("{:?}", e)))
//             .title("Error")
//             .button("Close", |s| {
//                 s.pop_layer();
//             })
//             .max_width(80),
//     );
// }

fn view_library(s: &mut Cursive, books: &[Book]) -> Result<(), Error> {
    let mut view = SelectView::new();

    for book in books {
        view.add_item(book.title.clone(), book.id);
    }

    view.set_on_submit(|s: &mut Cursive, id: &i64| {
        let b_id = *id;
        s.cb_sink()
            .send(Box::new(move |s| {
                update_view(s, Msg::GoChapterIndex(b_id, 1))
            }))
            .unwrap();
    });

    s.add_layer(
        Dialog::around(view.scrollable())
            .title("Library")
            .max_width(80),
    );

    Ok(())
}

fn view_chapter(s: &mut Cursive, chapter: &mut Chapter) -> Result<(), Error> {
    let styled_text = html_to_styled_string("body", &chapter.content[..])?;

    let mut dialog = Dialog::around(TextView::new(styled_text).scrollable());

    // if chapter.index + 1 < chapter.epub.get_num_pages() {
    dialog.add_button("Next", move |s| {
        s.cb_sink()
            .send(Box::new(move |s| update_view(s, Msg::NextChapter)))
            .unwrap();
    });
    // }

    if chapter.index > 0 {
        dialog.add_button("Prev", move |s| {
            s.cb_sink()
                .send(Box::new(move |s| update_view(s, Msg::PrevChapter)))
                .unwrap();
        });
    }

    dialog.add_button("TOC", move |s| {
        s.cb_sink()
            .send(Box::new(move |s| update_view(s, Msg::GoTOC)))
            .unwrap();
    });

    s.add_layer(dialog.max_width(80));

    Ok(())
}

fn view_toc(s: &mut Cursive, toc: &[Toc]) -> Result<(), Error> {
    let mut view = SelectView::new();

    for toc in toc {
        view.add_item(toc.title.clone(), toc.chapter_id);
    }

    view.set_on_submit(|s, id| {
        let c_id = *id;
        s.cb_sink()
            .send(Box::new(move |s| update_view(s, Msg::GoChapterId(c_id))))
            .unwrap();
    });

    s.add_layer(
        Dialog::around(view.scrollable())
            .title("Table of Contents")
            .max_width(80),
    );

    Ok(())
}
