use crate::Error;
use cursive::theme::{ColorStyle, Effect, Style};
use cursive::utils::markup::StyledString;
use ego_tree::iter::Edge;
// use epub::doc::EpubDoc;
use scraper::{ElementRef, Html, Selector};
// use std::fs::read;
// use std::io::Cursor;
// use std::path::{Path, PathBuf};
use wasmer_enumset::EnumSet;

// pub fn read_epub<P: AsRef<Path>>(path: P) -> Result<EpubDoc<Cursor<Vec<u8>>>, Error> {
//     let buff = read(&path)?;
//     let cursor = Cursor::new(buff);
//     let doc = EpubDoc::from_reader(cursor).map_err(|_| Error::UnableToParseEpub)?;
//
//     Ok(doc)
// }
//
// pub fn toc<P: AsRef<Path>>(path: P) -> Result<Vec<(String, PathBuf)>, Error> {
//     let buff = read(&path)?;
//     let cursor = Cursor::new(buff);
//     let doc = EpubDoc::from_reader(cursor).map_err(|_| Error::UnableToParseEpub)?;
//
//     let toc = doc
//         .toc
//         .iter()
//         .map(|nav| (nav.label.clone(), nav.content.clone()))
//         .collect::<Vec<(String, PathBuf)>>();
//
//     Ok(toc)
// }
//
// pub fn get_chapter_html<P: AsRef<Path>>(path: P, index: usize) -> Result<String, Error> {
//     let buff = read(&path)?;
//     let cursor = Cursor::new(buff);
//     let mut doc = EpubDoc::from_reader(cursor).map_err(|_| Error::UnableToParseEpub)?;
//
//     if index >= doc.spine.len() {
//         return Err(Error::InvalidSpineIndex(index));
//     }
//
//     let id = doc.spine[index].clone();
//     Ok(doc.get_resource_str(&id[..])?)
// }

// TODO: change this to a function that returns a linear layout so that
// alignment can be set on the text (such as horizontal lines).
pub fn html_to_styled_string(selector: &str, html: &str) -> Result<StyledString, Error> {
    let html = html.replace("\t", "    ");
    let html = html.replace("\u{9d}", "");
    let document = Html::parse_document(&html);
    let content_selector = Selector::parse(selector).map_err(|_| Error::UnableToParseHTML)?;

    let content = document
        .select(&content_selector)
        .collect::<Vec<ElementRef>>();

    let content = content
        .first()
        .ok_or_else(|| Error::UnableToFindSelector(selector.into()))?;

    #[derive(Copy, Clone, Debug, PartialEq)]
    enum Mode {
        Normal,
        Italic,
        Bold,
    }

    let (styled_string, _) = content.traverse().fold(
        (StyledString::new(), vec![Mode::Normal]),
        |(mut styled_string, mut modes), edge| {
            match edge {
                Edge::Open(node) => match &node.value() {
                    el if el.is_element() => {
                        let el = el.as_element().unwrap();
                        let local_name = el.name.local.to_string();
                        if local_name == "i" || local_name == "em" {
                            modes.push(Mode::Italic);
                        } else if local_name == "b" || local_name == "strong" {
                            modes.push(Mode::Bold);
                        } else if local_name == "br" || local_name == "p" || local_name == "div" {
                            styled_string.append_plain("\n");
                        } else if local_name == "hr" {
                            styled_string.append_plain("\n====================\n");
                        }
                    }
                    text if text.is_text() => {
                        let text = text.as_text().unwrap();
                        let mode = modes.last().unwrap();
                        match mode {
                            Mode::Normal => styled_string.append_plain(text.text.to_string()),
                            Mode::Italic => styled_string.append_styled(
                                text.text.to_string(),
                                Style {
                                    effects: EnumSet::only(Effect::Italic),
                                    color: ColorStyle::inherit_parent(),
                                },
                            ),
                            Mode::Bold => styled_string.append_styled(
                                text.text.to_string(),
                                Style {
                                    effects: EnumSet::only(Effect::Bold),
                                    color: ColorStyle::inherit_parent(),
                                },
                            ),
                        }
                    }
                    _ => {}
                },
                Edge::Close(node) => match &node.value() {
                    el if el.is_element() => {
                        let el = el.as_element().unwrap();
                        let local_name = el.name.local.to_string();
                        if local_name == "i" || local_name == "em" {
                            modes.pop();
                        } else if local_name == "p" || local_name == "div" {
                            styled_string.append_plain("\n");
                        }
                    }
                    _ => {}
                },
            }

            (styled_string, modes)
        },
    );

    Ok(styled_string)
}
