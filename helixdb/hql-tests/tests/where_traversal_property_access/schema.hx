N::Person {
    name: String,
    age: I32
}

E::Knows {
    From: Person,
    To: Person,
    Properties: {
        since: I32
    }
}
