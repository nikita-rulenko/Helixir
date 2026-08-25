N::User {
    name: String,
    INDEX birthday: Date,
    created_at: Date DEFAULT NOW,
}

N::Post {
    title: String,
    created_at: Date DEFAULT NOW,
}

E::Created {
    From: User,
    To: Post,
    Properties: { 
        created_at: Date,
    }
}

QUERY add(birthday: Date) => 
    user <- AddN<User>({name: "john", birthday: birthday})
    post <- AddN<Post>({title: "john"})
    edge <- AddE<Created>::From(user)::To(post)
    RETURN "success"

QUERY get_birthday_posts(birthday: Date, date: Date) =>
    posts <- N<User>({birthday: birthday})::Out<Created>::WHERE( _::InE<Created>::{created_at}::GTE(date))
    RETURN posts

QUERY get_birthday_posts_and(birthday: Date, date: Date) =>
    posts <- N<User>({birthday: birthday})::Out<Created>::WHERE(AND(_::{created_at}::GTE(date), _::InE<Created>::{created_at}::GTE(date)))
    RETURN posts

