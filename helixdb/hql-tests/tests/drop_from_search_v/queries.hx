V::User{
    name: String,
    age: I32,
}

QUERY dropUser(query: String) =>
    DROP SearchV<User>(Embed(query), 10)
    RETURN "success"





