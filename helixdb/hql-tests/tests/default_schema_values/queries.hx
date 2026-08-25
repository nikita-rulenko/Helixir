N::File9 {
    name: String,
    created_at: Date DEFAULT NOW,
}

E::EFile9 {
    From: File9,
    To: File9,
    Properties: {
        since: Date DEFAULT NOW,
    }
}


QUERY file9(date: Date) =>
    user <- AddN<File9>({name: "File9", created_at: "2021-01-01"})
    user2 <- AddN<File9>({name: "File9"})

    edge <- AddE<EFile9>::To(user2)::From(user)

    RETURN user, edge