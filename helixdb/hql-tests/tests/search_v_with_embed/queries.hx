N::File19 {
    name: String,
    age: I32,
}

E::Follows {
    From: File19,
    To: File19Vec,
}

V::File19Vec {
    name: String,
    age: I32,
}



QUERY file19(item: String) =>
    vec <- N<File19>::Out<Follows>::SearchV<File19Vec>(Embed(item), 10)
    RETURN vec