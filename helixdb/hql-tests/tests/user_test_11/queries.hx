N::User {
    INDEX username: String,
    INDEX phone_number: String,
    INDEX email: String,
    name: String,
    pfp_url: String,
    is_admin: Boolean DEFAULT false,
    is_verified: Boolean DEFAULT false,
    is_onboarded: Boolean DEFAULT false,
    created_at: Date DEFAULT NOW,
    updated_at: Date DEFAULT NOW,
}

E::UserToUserFollow {
    From: User,
    To: User,
    Properties: {
        since: Date DEFAULT NOW,
    }
}


QUERY GetUserWithFollowers (user_id: ID) =>
    user <- N<User>(user_id)
    followers <- user::In<UserToUserFollow>::RANGE(0, 50)::{id, username}
    follower_count <- user::In<UserToUserFollow>::COUNT
    RETURN {
        user: user,
        follower_count: follower_count,
        followers: followers
    }


QUERY GetUsersWithFollowers(start: I64, end: I64) =>
    results <- N<User>::RANGE(start, end)
    RETURN results::|u|{
        user: u,
        follower_count: u::In<UserToUserFollow>::COUNT,
        followers: u::In<UserToUserFollow>::RANGE(0, 50)::{id, username}
    }