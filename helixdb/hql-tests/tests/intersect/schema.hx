N::Tag {
    name: String
}

N::Article {
    title: String
}

E::HasTag {
    From: Article,
    To: Tag
}
