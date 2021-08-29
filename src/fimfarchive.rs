use crate::Error;
use regex::Captures;
use regex::Regex;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader, Lines};
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, Query, QueryParser, RangeQuery, TermQuery};
use tantivy::schema::*;
use tantivy::Index;
use tantivy::IndexReader;
use tantivy::ReloadPolicy;

pub fn load<P: AsRef<Path>>(
    fimfarchive_path: P,
    index_path: P,
) -> (FimfArchiveSchema, Index, IndexReader) {
    let schema = FimfArchiveSchema::new();

    let index = Index::create_in_dir(index_path, schema.schema.clone()).unwrap();
    // it's really the index.json path right now, need to change it to open the zip and get the index.json
    import_fimfarchive(fimfarchive_path, &index, &schema).unwrap();

    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommit)
        .try_into()
        .unwrap();

    (schema, index, reader)
}

pub fn open<P: AsRef<Path>>(path: P) -> (FimfArchiveSchema, Index, IndexReader) {
    let schema = FimfArchiveSchema::new();

    let index = Index::open_in_dir(path).unwrap();

    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommit)
        .try_into()
        .unwrap();

    (schema, index, reader)
}

type FileLines = Lines<BufReader<File>>;

fn file_lines<P: AsRef<Path>>(path: P) -> Result<FileLines, Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    Ok(reader.lines())
}

#[derive(Deserialize, Debug)]
struct FimfArchiveAuthor {
    id: i64,
    name: String,
    #[serde(rename = "bio.html")]
    bio: Option<String>,
}

#[derive(Deserialize, Debug)]
struct FimfArchiveTag {
    id: i64,
    name: String,
    #[serde(rename = "type")]
    category: String,
}

#[derive(Deserialize, Debug)]
struct FimfArchiveArchive {
    path: String,
}

#[derive(Deserialize, Debug)]
struct FimfArchiveBook {
    id: i64,
    archive: FimfArchiveArchive,
    author: FimfArchiveAuthor,
    title: Option<String>,
    #[serde(rename = "description_html")]
    description: Option<String>,
    #[serde(rename = "completion_status")]
    status: String,
    #[serde(rename = "content_rating")]
    rating: String,
    #[serde(rename = "num_likes")]
    likes: i64,
    #[serde(rename = "num_dislikes")]
    dislikes: i64,
    #[serde(rename = "num_words")]
    words: i64,
    tags: Vec<FimfArchiveTag>,
}

fn wilson_bounds(positive: f64, negative: f64) -> (f64, f64) {
    let total = positive + negative;

    let phat = positive / total;
    let z = 1.96;

    let a = phat + z * z / (2.0 * total);
    let b = z * ((phat * (1.0 - phat) + z * z / (4.0 * total)) / total).sqrt();
    let c = 1.0 + z * z / total;

    ((a - b) / c, (a + b) / c)
}

fn authors(
    mut input: String,
    schema: &FimfArchiveSchema,
) -> (String, Vec<(Occur, Box<dyn Query>)>) {
    let mut queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    let paren_escape_re = Regex::new(r#"\\\)"#).unwrap();

    let author_re = Regex::new(r#"author\(((?:\\\)|[^\)])+)\)"#).unwrap();
    let mut authors = Vec::new();

    input = author_re
        .replace_all(&input, |caps: &Captures| {
            let name = paren_escape_re.replace_all(&caps[1], |caps: &Captures| caps[1].to_string());
            authors.push(name.to_string());
            String::new()
        })
        .to_string();

    if authors.len() == 1 {
        let facet = Facet::from_path(&["author", &authors[0]]);
        println!("{}", facet);
        let term = Term::from_facet(schema.author, &facet);
        let query = TermQuery::new(term, IndexRecordOption::Basic);
        queries.push((Occur::Must, Box::new(query)));
    } else if authors.len() > 1 {
        let mut author_queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

        for author in authors {
            let facet = Facet::from_path(&["author", &author]);
            println!("{}", facet);
            let term = Term::from_facet(schema.author, &facet);
            let query = TermQuery::new(term, IndexRecordOption::Basic);
            author_queries.push((Occur::Should, Box::new(query)));
        }

        queries.push((Occur::Must, Box::new(BooleanQuery::new(author_queries))));
    }

    (input, queries)
}

fn tags(mut input: String, schema: &FimfArchiveSchema) -> (String, Vec<(Occur, Box<dyn Query>)>) {
    let mut queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    let paren_escape_re = Regex::new(r#"\\\)"#).unwrap();

    let mut all_tag_queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();
    // This first block is for excluded tags
    let ex_tag_re = Regex::new(r#"-#\(((?:\\\)|[^\)])+)\)"#).unwrap();
    let mut ex_tags = Vec::new();

    input = ex_tag_re
        .replace_all(&input, |caps: &Captures| {
            let name = paren_escape_re.replace_all(&caps[1], |caps: &Captures| caps[1].to_string());
            ex_tags.push(name.to_string());
            String::new()
        })
        .to_string();

    if ex_tags.len() != 0 {
        for ex_tag in ex_tags {
            let facet = Facet::from_path(&["tag", &ex_tag]);
            println!("ex {}", facet);
            let term = Term::from_facet(schema.tag, &facet);
            let query = TermQuery::new(term, IndexRecordOption::Basic);
            //ex_tag_queries.push((Occur::MustNot, Box::new(query)));
            all_tag_queries.push((Occur::MustNot, Box::new(query)));
        }
    }

    // This second block is for "or" tags (at least one of them must be present)
    let or_tag_re = Regex::new(r#"~#\(((?:\\\)|[^\)])+)\)"#).unwrap();
    let mut or_tags = Vec::new();

    input = or_tag_re
        .replace_all(&input, |caps: &Captures| {
            let name = paren_escape_re.replace_all(&caps[1], |caps: &Captures| caps[1].to_string());
            or_tags.push(name.to_string());
            String::new()
        })
        .to_string();

    if or_tags.len() != 0 {
        let mut or_tag_queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

        for or_tag in or_tags {
            let facet = Facet::from_path(&["tag", &or_tag]);
            println!("or {}", facet);
            let term = Term::from_facet(schema.tag, &facet);
            let query = TermQuery::new(term, IndexRecordOption::Basic);
            or_tag_queries.push((Occur::Should, Box::new(query)));
            //all_tag_queries.push((Occur::Should, Box::new(query)));
        }

        all_tag_queries.push((Occur::Must, Box::new(BooleanQuery::new(or_tag_queries))));
    }

    // This second block is for required tags
    let tag_re = Regex::new(r#"#\(((?:\\\)|[^\)])+)\)"#).unwrap();
    let mut tags = Vec::new();

    input = tag_re
        .replace_all(&input, |caps: &Captures| {
            let name = paren_escape_re.replace_all(&caps[1], |caps: &Captures| caps[1].to_string());
            tags.push(name.to_string());
            String::new()
        })
        .to_string();

    if tags.len() != 0 {
        for tag in tags {
            let facet = Facet::from_path(&["tag", &tag]);
            println!("{}", facet);
            let term = Term::from_facet(schema.tag, &facet);
            let query = TermQuery::new(term, IndexRecordOption::Basic);
            //tag_queries.push((Occur::Must, Box::new(query)));
            all_tag_queries.push((Occur::Must, Box::new(query)));
        }
    }

    // put the excluded and required tags together into one query
    if all_tag_queries.len() != 0 {
        queries.push((Occur::Must, Box::new(BooleanQuery::new(all_tag_queries))));
    }

    (input, queries)
}

fn words(mut input: String, schema: &FimfArchiveSchema) -> (String, Vec<(Occur, Box<dyn Query>)>) {
    let mut queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    let word_re = Regex::new(r#"words(>=|<=|>|<)([0-9]+)"#).unwrap();

    let mut lower = 0;
    let mut upper = i64::MAX;
    let mut filter_words = false;

    input = word_re
        .replace_all(&input, |caps: &Captures| {
            filter_words = true;
            let value = caps[2].parse::<i64>().unwrap();
            match &caps[1] {
                ">=" => {
                    if value > lower {
                        lower = value;
                    }
                }
                "<=" => {
                    if value + 1 < upper {
                        upper = value + 1;
                    }
                }
                ">" => {
                    if value + 1 > lower {
                        lower = value + 1;
                    }
                }
                "<" => {
                    if value < upper {
                        upper = value;
                    }
                }
                _ => unreachable!(),
            };
            String::new()
        })
        .to_string();

    if filter_words {
        let word_query = RangeQuery::new_i64(schema.words, lower..upper);
        queries.push((Occur::Must, Box::new(word_query)));
    }

    (input, queries)
}

fn likes(mut input: String, schema: &FimfArchiveSchema) -> (String, Vec<(Occur, Box<dyn Query>)>) {
    let mut queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    let like_re = Regex::new(r#"likes(>=|<=|>|<)([0-9]+)"#).unwrap();

    let mut lower = 0;
    let mut upper = i64::MAX;
    let mut filter_likes = false;

    input = like_re
        .replace_all(&input, |caps: &Captures| {
            filter_likes = true;
            let value = caps[2].parse::<i64>().unwrap();
            match &caps[1] {
                ">=" => {
                    if value > lower {
                        lower = value;
                    }
                }
                "<=" => {
                    if value + 1 < upper {
                        upper = value + 1;
                    }
                }
                ">" => {
                    if value + 1 > lower {
                        lower = value + 1;
                    }
                }
                "<" => {
                    if value < upper {
                        upper = value;
                    }
                }
                _ => unreachable!(),
            };
            String::new()
        })
        .to_string();

    if filter_likes {
        let like_query = RangeQuery::new_i64(schema.likes, lower..upper);
        queries.push((Occur::Must, Box::new(like_query)));
    }

    (input, queries)
}

fn dislikes(
    mut input: String,
    schema: &FimfArchiveSchema,
) -> (String, Vec<(Occur, Box<dyn Query>)>) {
    let mut queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    let dislike_re = Regex::new(r#"dislikes(>=|<=|>|<)([0-9]+)"#).unwrap();

    let mut lower = 0;
    let mut upper = i64::MAX;
    let mut filter_dislikes = false;

    input = dislike_re
        .replace_all(&input, |caps: &Captures| {
            filter_dislikes = true;
            let value = caps[2].parse::<i64>().unwrap();
            match &caps[1] {
                ">=" => {
                    if value > lower {
                        lower = value;
                    }
                }
                "<=" => {
                    if value + 1 < upper {
                        upper = value + 1;
                    }
                }
                ">" => {
                    if value + 1 > lower {
                        lower = value + 1;
                    }
                }
                "<" => {
                    if value < upper {
                        upper = value;
                    }
                }
                _ => unreachable!(),
            };
            String::new()
        })
        .to_string();

    if filter_dislikes {
        let dislike_query = RangeQuery::new_i64(schema.dislikes, lower..upper);
        queries.push((Occur::Must, Box::new(dislike_query)));
    }

    (input, queries)
}

fn wilson(mut input: String, schema: &FimfArchiveSchema) -> (String, Vec<(Occur, Box<dyn Query>)>) {
    let mut queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    let wilson_re = Regex::new(r#"wilson(>=|<=|>|<)([01].[0-9]+)"#).unwrap();

    let mut lower = 0.0;
    let mut upper = 1.0;
    let mut lower_inc = false;
    let mut upper_inc = false;
    let mut filter_wilson = false;

    input = wilson_re
        .replace_all(&input, |caps: &Captures| {
            filter_wilson = true;
            let value = caps[2].parse::<f64>().unwrap();
            match &caps[1] {
                ">=" => {
                    if value > lower {
                        lower = value;
                        lower_inc = true;
                    }
                }
                "<=" => {
                    if value < upper {
                        upper = value;
                        upper_inc = true;
                    }
                }
                ">" => {
                    if value > lower || (value == lower && lower_inc) {
                        lower = value;
                        lower_inc = false;
                    }
                }
                "<" => {
                    if value < upper || (value == upper && upper_inc) {
                        upper = value;
                        upper_inc = false;
                    }
                }
                _ => unreachable!(),
            };
            String::new()
        })
        .to_string();

    if filter_wilson {
        let lower = if lower_inc {
            std::ops::Bound::Included(lower)
        } else {
            std::ops::Bound::Excluded(lower)
        };
        let upper = if upper_inc {
            std::ops::Bound::Included(upper)
        } else {
            std::ops::Bound::Excluded(upper)
        };
        let wilson_query = RangeQuery::new_f64_bounds(schema.wilson, lower, upper);
        queries.push((Occur::Must, Box::new(wilson_query)));
    }

    (input, queries)
}

fn rating(mut input: String, schema: &FimfArchiveSchema) -> (String, Vec<(Occur, Box<dyn Query>)>) {
    let mut queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    let rating_re = Regex::new(r#"rating:(everyone|teen|mature)"#).unwrap();
    let mut ratings = Vec::new();

    input = rating_re
        .replace_all(&input, |caps: &Captures| {
            ratings.push(caps[1].to_string());
            String::new()
        })
        .to_string();

    for rating in ratings {
        let facet = Facet::from_path(&["rating", &rating]);
        println!("{}", facet);
        let term = Term::from_facet(schema.rating, &facet);
        let query = TermQuery::new(term, IndexRecordOption::Basic);
        queries.push((Occur::Must, Box::new(query)));
    }

    (input, queries)
}

fn status(mut input: String, schema: &FimfArchiveSchema) -> (String, Vec<(Occur, Box<dyn Query>)>) {
    let mut queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    let status_re = Regex::new(r#"status:(incomplete|complete|hiatus|cancelled)"#).unwrap();
    let mut statuses = Vec::new();

    input = status_re
        .replace_all(&input, |caps: &Captures| {
            statuses.push(caps[1].to_string());
            String::new()
        })
        .to_string();

    for status in statuses {
        let facet = Facet::from_path(&["status", &status]);
        println!("{}", facet);
        let term = Term::from_facet(schema.status, &facet);
        let query = TermQuery::new(term, IndexRecordOption::Basic);
        queries.push((Occur::Must, Box::new(query)));
    }

    (input, queries)
}

enum Order {
    Relevancy,
    Words,
    Likes,
    Dislikes,
    Wilson,
}

fn order(mut input: String) -> (String, Order) {
    let word_re = Regex::new(r#"order:(relevancy|words|likes|dislikes|wilson)"#).unwrap();

    let mut order = Order::Relevancy;

    input = word_re
        .replace_all(&input, |caps: &Captures| {
            order = match &caps[1] {
                "relevancy" => Order::Relevancy,
                "words" => Order::Words,
                "likes" => Order::Likes,
                "dislikes" => Order::Dislikes,
                "wilson" => Order::Wilson,
                _ => unreachable!(),
            };
            String::new()
        })
        .to_string();

    (input, order)
}

type FilterFn = fn(String, &FimfArchiveSchema) -> (String, Vec<(Occur, Box<dyn Query>)>);

pub fn search(
    mut input: String,
    limit: usize,
    index: &Index,
    schema: &FimfArchiveSchema,
    reader: &IndexReader,
) {
    let searcher = reader.searcher();

    let mut queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    let filters: Vec<FilterFn> = vec![
        authors, tags, words, likes, dislikes, wilson, rating, status
    ];

    for filter in filters {
        let (new_input, mut filter_queries) = filter(input, schema);
        queries.append(&mut filter_queries);
        input = new_input;
    }

    let (input, order) = order(input);

    let input = input.trim_start().trim_end().to_string();
    println!("input: [{}]", input);
    if input.len() != 0 {
        let query_parser = QueryParser::for_index(&index, vec![schema.title, schema.description]);
        let text_query = query_parser.parse_query(&input).unwrap();

        queries.push((Occur::Must, Box::new(text_query)));
    }

    let query = BooleanQuery::new(queries);
    println!("{:?}", query);
    use tantivy::DocAddress;

    let docs: Vec<tantivy::DocAddress> = match order {
        Order::Relevancy => {
            let collector = TopDocs::with_limit(limit);
            let top_docs: Vec<(f32, tantivy::DocAddress)> =
                searcher.search(&query, &collector).unwrap();

            top_docs
                .into_iter()
                .map(|(_score, doc_address): (f32, DocAddress)| doc_address)
                .collect()
        }
        Order::Words => {
            let collector = TopDocs::with_limit(limit).order_by_fast_field(schema.words);
            let top_docs: Vec<(i64, tantivy::DocAddress)> =
                searcher.search(&query, &collector).unwrap();

            top_docs
                .into_iter()
                .map(|(_score, doc_address): (i64, DocAddress)| doc_address)
                .collect()
        }
        Order::Likes => {
            let collector = TopDocs::with_limit(limit).order_by_fast_field(schema.likes);
            let top_docs: Vec<(i64, tantivy::DocAddress)> =
                searcher.search(&query, &collector).unwrap();

            top_docs
                .into_iter()
                .map(|(_score, doc_address): (i64, DocAddress)| doc_address)
                .collect()
        }
        Order::Dislikes => {
            let collector = TopDocs::with_limit(limit).order_by_fast_field(schema.dislikes);
            let top_docs: Vec<(i64, tantivy::DocAddress)> =
                searcher.search(&query, &collector).unwrap();

            top_docs
                .into_iter()
                .map(|(_score, doc_address): (i64, DocAddress)| doc_address)
                .collect()
        }
        Order::Wilson => {
            let collector = TopDocs::with_limit(limit).order_by_fast_field(schema.wilson);
            let top_docs: Vec<(f64, tantivy::DocAddress)> =
                searcher.search(&query, &collector).unwrap();

            top_docs
                .into_iter()
                .map(|(_score, doc_address): (f64, DocAddress)| doc_address)
                .collect()
        }
    };

    //let top_docs: Vec<(f32, tantivy::DocAddress)> = searcher.search(&query, &collector).unwrap();

    println!("There are {} results.", docs.len());
    for doc_address in docs {
        let retrieved_doc = searcher.doc(doc_address).unwrap();
        //println!("{} {}", score, schema.schema.to_json(&retrieved_doc));
        println!(
            "{:?} by {:?} words {:?} likes {:?} dislikes {:?} wilson {:?} status {:?} rating {:?}",
            retrieved_doc
                .get_first(schema.title)
                .unwrap()
                .text()
                .unwrap(),
            retrieved_doc
                .get_first(schema.author)
                .unwrap()
                .path()
                .unwrap(),
            retrieved_doc
                .get_first(schema.words)
                .unwrap()
                .i64_value()
                .unwrap(),
            retrieved_doc
                .get_first(schema.likes)
                .unwrap()
                .i64_value()
                .unwrap(),
            retrieved_doc
                .get_first(schema.dislikes)
                .unwrap()
                .i64_value()
                .unwrap(),
            retrieved_doc
                .get_first(schema.wilson)
                .unwrap()
                .f64_value()
                .unwrap(),
            retrieved_doc
                .get_first(schema.status)
                .unwrap()
                .path()
                .unwrap(),
            retrieved_doc
                .get_first(schema.rating)
                .unwrap()
                .path()
                .unwrap(),
            //retrieved_doc.get_all(schema.tag).map(|f| f.path().unwrap()).collect::<Vec<String>>(),
        );
    }
}

pub struct FimfArchiveSchema {
    schema: Schema,
    title: Field,
    description: Field,
    author: Field,
    path: Field,
    likes: Field,
    dislikes: Field,
    words: Field,
    wilson: Field,
    status: Field,
    rating: Field,
    tag: Field,
}

impl FimfArchiveSchema {
    fn new() -> Self {
        let mut schema_builder = Schema::builder();
        schema_builder.add_text_field("title", TEXT | STORED);
        schema_builder.add_text_field("description", TEXT | STORED);
        schema_builder.add_facet_field("author", INDEXED | STORED);
        schema_builder.add_text_field("path", TEXT | STORED);
        schema_builder.add_i64_field("likes", INDEXED | STORED | FAST);
        schema_builder.add_i64_field("dislikes", INDEXED | STORED | FAST);
        schema_builder.add_i64_field("words", INDEXED | STORED | FAST);
        schema_builder.add_f64_field("wilson", INDEXED | STORED | FAST);
        schema_builder.add_facet_field("status", INDEXED | STORED);
        schema_builder.add_facet_field("rating", INDEXED | STORED);
        schema_builder.add_facet_field("tag", INDEXED | STORED);
        let schema = schema_builder.build();

        FimfArchiveSchema {
            schema: schema.clone(),
            title: schema.get_field("title").unwrap(),
            description: schema.get_field("description").unwrap(),
            author: schema.get_field("author").unwrap(),
            path: schema.get_field("path").unwrap(),
            likes: schema.get_field("likes").unwrap(),
            dislikes: schema.get_field("dislikes").unwrap(),
            words: schema.get_field("words").unwrap(),
            wilson: schema.get_field("wilson").unwrap(),
            status: schema.get_field("status").unwrap(),
            rating: schema.get_field("rating").unwrap(),
            tag: schema.get_field("tag").unwrap(),
        }
    }
}

fn import_fimfarchive<P: AsRef<Path>>(
    path: P,
    index: &Index,
    schema: &FimfArchiveSchema,
) -> Result<(), Error> {
    let mut index_writer = index.writer(16_000_000).unwrap();

    for line in file_lines(path).unwrap() {
        let line = line.unwrap();
        if line.len() != 1 {
            // ignore the object key and trailing comma
            let mut start = 0;
            for j in 0..line.len() {
                if line.as_bytes()[j] == b'{' {
                    start = j;
                    break;
                }
            }
            let end = if line.as_bytes()[line.len() - 1] == b'}' {
                line.len()
            } else {
                line.len() - 1
            };
            let object = &line[start..end];

            let book: FimfArchiveBook = serde_json::from_str(object).unwrap();

            let mut doc = Document::default();
            if let Some(t) = book.title {
                doc.add_text(schema.title, t);
            } else {
                doc.add_text(schema.title, "UNTITLED");
            }
            if let Some(d) = book.description {
                doc.add_text(schema.description, d);
            } else {
                doc.add_text(schema.description, "");
            }

            doc.add_facet(schema.author, &format!("/author/{}", book.author.name));
            doc.add_text(schema.path, book.archive.path);
            doc.add_i64(schema.likes, book.likes);
            doc.add_i64(schema.dislikes, book.dislikes);
            doc.add_i64(schema.words, book.words);

            if book.likes > 0 && book.dislikes >= 0 {
                let (lower, _upper) = wilson_bounds(book.likes as f64, book.dislikes as f64);
                doc.add_f64(schema.wilson, lower);
            } else {
                doc.add_f64(schema.wilson, 0.0);
            }

            doc.add_facet(schema.status, &format!("/status/{}", book.status));
            doc.add_facet(schema.rating, &format!("/rating/{}", book.rating));

            for t in book.tags {
                doc.add_facet(schema.tag, &format!("/tag/{}", t.name));
            }

            index_writer.add_document(doc);
        }
    }

    index_writer.commit().unwrap();
    Ok(())
}
