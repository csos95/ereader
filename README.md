- book
    - [x] scan epub directory
    - [x] get metadata from epubs
    - [x] add to database
    - [x] compare found books to library books and only insert new ones
        hash all but the path, put books in a hashmap of hash => book
            then, find the difference between the two maps and
            for found books not in library, add to library
            for library books not found, display errors
            for found books in library, check that the paths match, if they do not, update the path in the library
- library
    - [ ] list books in library
    - [ ] select book
- reader (should mostly be copy from first project, might change how chapter is gotten)
    - [ ] get chapter from epub
    - [ ] parse and style the html
    - [ ] display in scroll view
    - [ ] prev/next chapter
    - [ ] bookmark
- bookmark
    - [ ] list
    - [ ] select to go to chapter/location in book
    - [ ] delete
- fimfarchive (first two should mostly be copy from old project)
    - [ ] get author, tag, story, and story_tag from index.json
    - [ ] search for fics
    - [ ] copy epub to library
- bookshelf
    - [ ] have add books menu that lists books not currently in
    - [ ] have add to bookshelf button on books
    - [ ] have remove button in bookshelf