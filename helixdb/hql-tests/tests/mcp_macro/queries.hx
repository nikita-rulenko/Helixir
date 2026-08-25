N::User{
    name: String,
}

#[mcp]
QUERY get_user_name(user_id: ID) =>
    user <- N<User>(user_id)
    RETURN user
