N::Chapter {
    chapter_index: I64
}

N::SubChapter {
    title: String,
    content: String
}

E::Contains {
    From: Chapter,
    To: SubChapter,
    Properties: {
    }
}

V::Embedding {
    chunk: String
}

E::EmbeddingOf {
    From: SubChapter,
    To: Embedding,
    Properties: {
        chunk: String
    }
}

