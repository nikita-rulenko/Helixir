N::UserFile17 {
    name: String,
    age: U32
}

QUERY update_user(userID: ID, name: String, age: U32) =>
    updatedUsers <- N<UserFile17>(userID)::UPDATE({name: name, age: age})
    RETURN updatedUsers