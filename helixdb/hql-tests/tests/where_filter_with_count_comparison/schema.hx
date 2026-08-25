N::User {
    name: String,
    age: U32,
    email: String,
    created_at: I32,
    updated_at: I32,
}   

N::Post {
    content: String,
    created_at: I32,
    updated_at: I32,
}

E::Follows {
    From: User,
    To: User,
    Properties: {
        since: I32,
    }
}

E::Created {
    From: User,
    To: Post,
    Properties: {
        created_at: I32,
    }
}