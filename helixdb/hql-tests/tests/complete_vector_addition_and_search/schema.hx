N::Doc {
    content: String,
    INDEX number: I32
}
    
V::Embedding {
    chunk: String,
    chunk_id: I32,
    number: I32,
    reference: String
}

N::Chunk {
    content: String,
}

E::EmbeddingOf {
    From: Doc,
    To: Embedding, 
}

// N::User {
//     name: String,
//     age: I32
// }
// 
// E::Knows {
//     From: User,
//     To: User,
//     Properties: {
//         since: I32,
//     }
// }


