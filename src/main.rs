#![allow(dead_code)]

mod fimfarchive;
mod library;
mod scan;
mod tui;

use cursive::{Cursive, CursiveExt};
// use sqlx::SqlitePool;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("sqlx error {0}")]
    SqlxError(sqlx::Error),
    #[error("unable to parse epub")]
    UnableToParseEpub,
    #[error("missing metadata tag {0}")]
    MissingMetadata(String),
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
    #[error("url parse error {0}")]
    UrlParseError(url::ParseError),
    #[error("epub missing resource listed in table of contents")]
    EpubMissingTocResource,
    #[error("debug message {0}")]
    DebugMsg(String),
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

impl From<url::ParseError> for Error {
    fn from(e: url::ParseError) -> Self {
        Error::UrlParseError(e)
    }
}

#[async_std::main]
async fn main() {
    // // what is needed for loading the index and what is needed for searching?
    // // for loading, the location of the fimfarchive.zip and the directory for the index
    // // for searching, the directory for the index

    // //let (schema, index, reader) = fimfarchive::load("index.json", "index");
    // let (schema, index, reader) = fimfarchive::open("index");

    // println!("What is your query?");

    // let stdin = std::io::stdin();
    // let input = stdin.lock().lines().next().unwrap().unwrap();

    // println!("Results limit?");

    // let stdin = std::io::stdin();
    // let limit_str = stdin.lock().lines().next().unwrap().unwrap();
    // let limit: usize = limit_str.parse().expect("expected a usize");

    // fimfarchive::search(input, limit, &index, &schema, &reader);

    // let pool = SqlitePool::connect("ereader.sqlite").await.unwrap();
    // let start = chrono::Utc::now();
    // scan::scan(&pool, "epub").await.unwrap();
    // let end = chrono::Utc::now();
    // println!("start {}\nend {}\ndiff {}", start, end, end - start);
    // pool.close().await;

    let mut siv = Cursive::new();

    let model = tui::init().await.unwrap();
    tui::view(&mut siv, &model);
    siv.set_user_data(model);

    siv.add_global_callback('q', |s| {
        tui::cleanup(s);
    });
    siv.add_global_callback('l', |s| {
        s.cb_sink()
            .send(Box::new(move |s| tui::update_view(s, tui::Msg::GoLibrary)))
            .unwrap();
    });
    siv.run();
}
