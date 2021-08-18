use crate::library;
use crate::Error;
use epub::doc::EpubDoc;
use itertools::{Either, Itertools};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::collections::HashSet;
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
    pub hash: String,
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

    // put the found books into a set and map
    let (f1, f2) = found_books.into_iter().tee();

    let found_hashes = f1.into_iter().fold(HashSet::new(), |mut set, book| {
        set.insert(book.hash.clone());
        set
    });

    let found_map = f2.into_iter().fold(HashMap::new(), |mut map, book| {
        map.insert(book.hash.clone(), book);
        map
    });

    // get the books in the library
    let library_books = library::get_books(pool).await?;

    // put the library books in a set
    let library_hashes = library_books
        .into_iter()
        .fold(HashSet::new(), |mut set, book| {
            set.insert(book.hash.clone());
            set
        });

    // find the difference of the sets found_set - library_set to get the new books
    let new_hashes = found_hashes.difference(&library_hashes);

    // insert the new books
    for hash in new_hashes {
        let new_book = found_map.get(hash).unwrap();
        library::insert_book(pool, new_book).await?;
    }

    // print any errors in scanning (might just be permissions errors so don't want to just crash)
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
    let hash = blake3::hash(buff.as_slice());
    let cursor = Cursor::new(buff);
    let doc = EpubDoc::from_reader(cursor).map_err(|_| Error::UnableToParseEpub)?;

    Ok(SourceBook {
        identifier: get_metadata(&path, &doc, "title")?,
        language: get_metadata(&path, &doc, "language")?,
        title: get_metadata(&path, &doc, "title")?,
        creator: doc.mdata("creator"),
        description: doc.mdata("description"),
        publisher: doc.mdata("publisher"),
        hash: hash.to_string(),
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
