N::File5 {
    name: String,
    age: I32,
}


E::EdgeFile5 {
    From: File5,
    To: File5,
}


QUERY file5() =>
    user <- AddN<File5>({name: "John", age: 20})
    user2 <- N<File5>::Out<EdgeFile5>
    RETURN user::{
        username: name,
        age: 21
    }
