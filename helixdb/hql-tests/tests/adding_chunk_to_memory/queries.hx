 QUERY AddChunkToMemory(
      text_content: String,
      order_in_document: U32,
      metadata: String,
      created_at: String
  ) =>
    
      chunk <- AddV<Chunk>(Embed(text_content), {
          text_content: text_content,
          order_in_document: order_in_document,
          metadata: metadata,
          created_at: created_at
      })

      memory <- AddN<Memory>({
        content: text_content,
        original_input: text_content,
        title: text_content,
        content_type: "page",
        url: text_content,
        metadata: metadata,
        chunk_count: 1,
        created_at: created_at,
        updated_at: created_at
      })
      has_chunk <- AddE<HasChunk>({
          similarity_score: 0.0,
          created_at: created_at
      })::From(memory)::To(chunk)

      RETURN chunk

