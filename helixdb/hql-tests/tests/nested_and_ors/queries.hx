QUERY GetFilteredUsers () =>
    users <- N<User>::WHERE(
        AND(
            _::{age}::GT(18),
            OR(_::{name}::EQ("Alice"), _::{name}::EQ("Bob"))
        )
    )
    RETURN users

QUERY CreateUser (name: String, age: U8, email: String) =>
    user <- AddN<User>({
        name: name,
        age: age,
        email: email
    })
    RETURN user

QUERY GetUserDisplayInfo () =>
    users <- N<User>::RANGE(0, 10)
    RETURN users::{displayName: name, userAge: age}

QUERY GetUsersWithAlias () =>
    users <- N<User>::RANGE(0, 5)
    RETURN users::{
        userID: ID,
        ..
    }
