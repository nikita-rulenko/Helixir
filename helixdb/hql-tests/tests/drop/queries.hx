N::File15 {
    name: String,
    age: I32,
}

E::Follows {
    From: File15,
    To: File15,
}

QUERY file15() =>
    DROP N<File15>
    RETURN "success"

QUERY file15_2(userID: ID) =>
    DROP N<File15>(userID)::Out<Follows>
    RETURN NONE

QUERY file15_3(userID: ID) =>
    DROP N<File15>(userID)
    RETURN NONE