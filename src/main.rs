mod epub;
mod library;
mod scan;

use crate::epub::html_to_styled_string;
use ::epub::doc::EpubDoc;
use async_std::task;
use cursive::traits::Scrollable;
use cursive::view::Resizable;
use cursive::views::{Dialog, SelectView, TextView};
use cursive::{Cursive, CursiveExt};
use library::Book;
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

struct Chapter {
    epub: EpubDoc<std::io::Cursor<Vec<u8>>>,
    path: String,
    index: usize,
}

struct TableOfContents {
    chapter: Chapter,
    toc: Vec<(String, usize)>,
}

enum Page {
    Library(Vec<Book>),
    Chapter(Chapter),
    TableOfContents(TableOfContents),
}

enum Msg {
    GoLibrary,
    GoChapter(String, usize),
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
        (
            Msg::GoChapter(new_path, new_index),
            Page::Chapter(Chapter {
                path,
                mut epub,
                mut index,
            }),
        ) if new_path == path => {
            epub.set_current_page(index).expect("invalid page");
            index = new_index;
            Page::Chapter(Chapter { epub, path, index })
        }
        (Msg::GoChapter(path, index), _) => {
            let mut epub = epub::read_epub(&path)?;
            epub.set_current_page(index).expect("invalid page");
            Page::Chapter(Chapter { epub, path, index })
        }
        (Msg::NextChapter, Page::Chapter(mut chapter)) => {
            chapter
                .epub
                .set_current_page(chapter.index + 1)
                .expect("invalid page");
            chapter.index += 1;
            Page::Chapter(chapter)
        }
        (Msg::PrevChapter, Page::Chapter(mut chapter)) => {
            chapter
                .epub
                .set_current_page(chapter.index - 1)
                .expect("invalid page");
            chapter.index -= 1;
            Page::Chapter(chapter)
        }
        (Msg::GoTOC, Page::Chapter(chapter)) => {
            let toc = epub::toc(&chapter.path)?
                .into_iter()
                .map(|(title, path)| {
                    let index = chapter.epub.resource_uri_to_chapter(&path).unwrap();
                    (title, index)
                })
                .collect::<Vec<(String, usize)>>();
            Page::TableOfContents(TableOfContents { chapter, toc })
        }
        (_msg, page) => page,
    };

    Ok(model)
}

fn view(s: &mut Cursive, model: &mut Model) -> Result<(), Error> {
    match &mut model.page {
        Page::Chapter(chapter) => view_chapter(s, chapter)?,
        Page::Library(books) => view_library(s, &books)?,
        Page::TableOfContents(toc) => view_toc(s, &toc)?,
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

fn error(s: &mut Cursive, e: Error) {
    s.add_layer(
        Dialog::around(TextView::new(format!("{:?}", e)))
            .title("Error")
            .button("Close", |s| {
                s.pop_layer();
            })
            .max_width(80),
    );
}

fn view_library(s: &mut Cursive, books: &Vec<Book>) -> Result<(), Error> {
    let mut view = SelectView::new();

    for book in books {
        view.add_item(book.title.clone(), book.path.clone());
    }

    view.set_on_submit(|s: &mut Cursive, path: &String| {
        let c_path = path.to_string();
        s.cb_sink()
            .send(Box::new(move |s| update_view(s, Msg::GoChapter(c_path, 0))))
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
    let html = chapter.epub.get_current_str()?;
    let styled_text = html_to_styled_string("body", &html[..])?;

    let mut dialog = Dialog::around(TextView::new(styled_text).scrollable());

    if chapter.index + 1 < chapter.epub.get_num_pages() {
        dialog.add_button("Next", move |s| {
            s.cb_sink()
                .send(Box::new(move |s| update_view(s, Msg::NextChapter)))
                .unwrap();
        });
    }

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

fn view_toc(s: &mut Cursive, toc: &TableOfContents) -> Result<(), Error> {
    let mut view = SelectView::new();

    for (title, index) in &toc.toc {
        view.add_item(title.to_string(), (toc.chapter.path.clone(), *index));
    }

    view.set_on_submit(|s, (path, index)| {
        let c_path = path.to_string();
        let c_index = *index;
        s.cb_sink()
            .send(Box::new(move |s| {
                update_view(s, Msg::GoChapter(c_path, c_index))
            }))
            .unwrap();
    });

    s.add_layer(
        Dialog::around(view.scrollable())
            .title("Table of Contents")
            .max_width(80),
    );

    Ok(())
}
