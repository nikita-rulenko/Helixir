N::UserFile18 {
    name: String,
    age: U32
}

QUERY update_user(userID: ID, name: String, age: U32) =>
    updatedUsers <- N<UserFile18>(userID)::WHERE(_::{age}::EQ(age))
    RETURN updatedUsers