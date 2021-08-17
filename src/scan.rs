use crate::library;
use crate::Error;
use epub::doc::EpubDoc;
use itertools::{Either, Itertools};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::fs::{read, read_dir};
use std::io::Cursor;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct SourceBook {
    pub identifier: String,
    pub language: String,
    pub title: String,
    pub creator: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub path: String,
}

#[derive(Clone, Debug)]
pub struct SourceChapter {
    pub index: usize,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct TOC {
    pub index: usize,
    pub title: String,
    pub spine_index: usize,
}

pub async fn scan<P: AsRef<Path>>(pool: &SqlitePool, path: P) -> Result<(), Error> {
    // get the books in the epub directory
    let (found_books, errors) = scan_directory(path)?;
    let found_map = found_books
        .into_iter()
        .fold(HashMap::new(), |mut map, book| {
            map.insert(
                (
                    book.identifier.clone(),
                    book.language.clone(),
                    book.title.clone(),
                    book.creator.clone(),
                    book.description.clone(),
                    book.publisher.clone(),
                ),
                book,
            );
            map
        });

    // get the books in the library
    let library_books = library::get_books(pool).await?;
    let library_map = library_books
        .into_iter()
        .fold(HashMap::new(), |mut map, book| {
            map.insert(
                (
                    book.identifier.clone(),
                    book.language.clone(),
                    book.title.clone(),
                    book.creator.clone(),
                    book.description.clone(),
                    book.publisher.clone(),
                ),
                book,
            );
            map
        });

    let mut new_books = Vec::new();
    let mut lost_books = Vec::new();
    let mut moved_books = Vec::new();

    // figure out the new and moved books
    for (key, book) in &found_map {
        match library_map.get(key) {
            Some(library_book) => {
                if book.path != library_book.path {
                    let mut moved_book = library_book.clone();
                    moved_book.path = book.path.clone();
                    moved_books.push(moved_book);
                }
            }
            None => {
                new_books.push(book.clone());
            }
        }
    }

    // figure out the lost books
    for (key, book) in &library_map {
        if found_map.get(key).is_none() {
            lost_books.push(book.clone());
        }
    }

    for book in &new_books {
        println!("New Book: {} at {}", book.title, book.path);
        library::insert_book(pool, book).await.unwrap();
    }

    for book in &lost_books {
        println!("Lost Book: {} from {}", book.title, book.path);
    }

    for book in &moved_books {
        println!("Moved Book: {} to {}", book.title, book.path);
        library::update_book_path(pool, book).await.unwrap();
    }

    for error in errors {
        println!("{:?}", error);
    }

    Ok(())
}

pub fn scan_directory<P: AsRef<Path>>(path: P) -> Result<(Vec<SourceBook>, Vec<Error>), Error> {
    // get books in current directory
    let (mut books, mut errors): (Vec<SourceBook>, Vec<Error>) = read_dir(&path)?
        .filter_map(|entry| entry.ok())
        .filter(|dir| dir.path().extension().unwrap_or_default() == "epub")
        .partition_map(|dir| match scan_book(dir.path()) {
            Ok(book) => Either::Left(book),
            Err(e) => Either::Right(e),
        });

    // get paths of sub directories
    let sub_paths = read_dir(&path)?
        .filter_map(|entry| entry.ok())
        .filter_map(|dir| {
            if let Ok(file_type) = dir.file_type() {
                if file_type.is_dir() || file_type.is_symlink() {
                    return Some(dir.path());
                }
            }
            None
        });

    // scan the sub directories
    for path in sub_paths {
        let (mut sub_books, mut sub_errors) = scan_directory(path)?;
        books.append(&mut sub_books);
        errors.append(&mut sub_errors)
    }

    Ok((books, errors))
}

fn scan_book<P: AsRef<Path>>(path: P) -> Result<SourceBook, Error> {
    let buff = read(&path)?;
    let cursor = Cursor::new(buff);
    let doc = EpubDoc::from_reader(cursor).map_err(|_| Error::UnableToParseEpub)?;

    Ok(SourceBook {
        identifier: get_metadata(&path, &doc, "title")?,
        language: get_metadata(&path, &doc, "language")?,
        title: get_metadata(&path, &doc, "title")?,
        creator: doc.mdata("creator"),
        description: doc.mdata("description"),
        publisher: doc.mdata("publisher"),
        path: path.as_ref().to_string_lossy().to_string(),
    })
}

fn get_metadata<P: AsRef<Path>>(
    path: P,
    doc: &EpubDoc<Cursor<Vec<u8>>>,
    tag: &str,
) -> Result<String, Error> {
    doc.mdata(tag).ok_or_else(|| {
        Error::MissingMetadata(path.as_ref().to_string_lossy().to_string(), tag.to_string())
    })
}
