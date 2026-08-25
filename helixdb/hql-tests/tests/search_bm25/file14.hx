N::File14 {
    name: String,
    age: I32,
}

QUERY file14() =>
    res <- SearchBM25<File14>("John", 10)
    RETURN res


QUERY search_with_k(k: I32) =>
    res <- SearchBM25<File14>("John", k)
    RETURN res