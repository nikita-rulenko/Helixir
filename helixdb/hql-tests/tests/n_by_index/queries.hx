N::File9 {
    INDEX name: String,
    INDEX age: I32,
    INDEX count: F32,
    other_field: String,
}


QUERY file9(name: String, id: ID) =>
    user <- N<File9>(id)
    node <- N<File9>({name: name})
    node_by_name <- N<File9>({count: 24.5})
    RETURN user, node, node_by_name