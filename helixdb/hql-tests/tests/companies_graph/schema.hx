// ─── Node types ──────────────────────────────────────────────
N::Company {
    INDEX company_number: String,
    total_filings: I32,
    ingested_filings: I32,
}

// ─── Vector types ──────────────────────────────────────────────

V::DocumentEmbedding {
    text:        String,     // The actual content
    chunk_id:    String,     // Unique chunk identifier
    page_number: U16,        // Which page this chunk came from
    reference:   String,     // Formatted reference for citations
    source_link: String,    // Original URL/link
    source_date: String,     // Date of the filing
}

// ─── Edge types ──────────────────────────────────────────────
// TODO: add cotegory and double check date
E::DocumentEdge {
    From: Company,
    To:   DocumentEmbedding,
    Properties: {
        filing_id:   String,     // Filter by specific document
        category:    String,     // Filter by filing type (e.g., "accounts", "confirmation-statement")
        subcategory: String, // Filter by filing subtype
        date:        String,     // Filter by date range (recent filings)
        description: String,     // Human readable filing description
    }
}