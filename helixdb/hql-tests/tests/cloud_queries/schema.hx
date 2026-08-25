N::Cluster {
    region: String,
    api_url: String DEFAULT "",
    status: String DEFAULT "pending",
    created_at: Date DEFAULT NOW,
    updated_at: Date DEFAULT NOW,
}

N::Instance {
    region: String,
    instance_type: String,
    storage_gb: I64,
    ram_gb: I64,
    status: String DEFAULT "pending",
    api_url: String DEFAULT "",
    created_at: Date DEFAULT NOW,
    updated_at: Date DEFAULT NOW,

}

N::User {
    INDEX gh_id: U64,
    gh_login: String,
    name: String,
    email: String,
    created_at: Date DEFAULT NOW,
    updated_at: Date DEFAULT NOW,
}

E::CreatedCluster {
    From: User,
    To: Cluster,
}

E::CreatedInstance {
    From: Cluster,
    To: Instance,
}
