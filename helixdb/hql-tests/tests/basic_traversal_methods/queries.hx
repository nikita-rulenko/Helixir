N::File4 {
    name: String,
    age: I32,
}


E::EdgeFile4 {
    From: File4,
    To: File4,
}


QUERY file4() =>
    user <- AddN<File4>({name: "John", age: 20})
    user2 <- N<File4>::Out<EdgeFile4>
    user3 <- N<File4>::In<EdgeFile4>
    edge1 <- N<File4>::OutE<EdgeFile4>
    edge2 <- N<File4>::InE<EdgeFile4>

    user4 <- user2::Out<EdgeFile4>
    user5 <- user3::In<EdgeFile4>
    edge3 <- user2::OutE<EdgeFile4>
    edge4 <- user3::InE<EdgeFile4>

    user6 <- edge3::FromN
    user7 <- edge4::ToN

    RETURN user






