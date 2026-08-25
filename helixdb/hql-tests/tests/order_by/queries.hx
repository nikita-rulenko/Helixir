N::User {
    name: String,
    age: I32,
    created_at: Date DEFAULT NOW,
}

E::Knows {
    From: User,
    To: User,
    Properties: {
        since: Date,
    }
}

QUERY AddUser() =>
    user <- AddN<User>({name: "john", age: 20})
    RETURN user

QUERY GetOrder() =>
    userByAge <- N<User>::ORDER<Desc>(_::{created_at})
    userByAge2 <- N<User>::ORDER<Asc>(_::{created_at})
    RETURN userByAge, userByAge2

