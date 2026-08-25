N::File9 {
    INDEX name: String,
    INDEX age: I32,
    INDEX count: F32 DEFAULT 0.0,
    other_field: String,
}


QUERY file9(name: String, id: ID) =>
    user <- AddN<File9>({name: name, age: 10, other_field: "other_field"})
    user_by_id <- N<File9>(id)
    node <- N<File9>({name: name})
    node_by_name <- N<File9>({count: 24.5})
    RETURN user, node, node_by_name