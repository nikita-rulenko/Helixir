QUERY filter_users() =>
    users <- N<User>::WHERE(_::In<Follows>::COUNT::GT(1))::Out<Follows>
    RETURN users