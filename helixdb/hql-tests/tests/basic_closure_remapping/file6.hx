N::File6 {
    name: String,
    age: I32,
}


E::EdgeFile6 {
    From: File6,
    To: File6,
}


QUERY file6() =>
    user <- AddN<File6>({name: "John", age: 20})
    user2 <- N<File6>::Out<EdgeFile6>
    RETURN user::|u|{
        username: u::{name},
        age: age
    }
