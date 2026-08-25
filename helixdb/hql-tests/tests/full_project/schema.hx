N::User {
    UNIQUE INDEX github_id: U64,
    github_login: String,
    UNIQUE INDEX github_name: String,
    UNIQUE INDEX github_email: String,
    created_at: Date DEFAULT NOW,
    updated_at: Date DEFAULT NOW,
}

N::Workspace {
    UNIQUE INDEX url_slug: String,
    name: String,
    workspace_type: String DEFAULT "personal",
    icon: String DEFAULT "",
    plan: String DEFAULT "none",
    created_at: Date DEFAULT NOW,
    updated_at: Date DEFAULT NOW,
}

N::Project {
    name: String,
    created_at: Date DEFAULT NOW,
    updated_at: Date DEFAULT NOW,
}

N::ObjectCluster {
    cluster_name: String,
    // NOTE: No build_mode - inferred from availability_mode (dev mode = dev build, ha mode = release build)
    availability_mode: String DEFAULT "dev",      // "dev" | "ha"
    min_instances: U64 DEFAULT 1,
    max_instances: U64 DEFAULT 1,
    gateway_node_type: String DEFAULT "",         // "GW-20" | "GW-40" | etc.
    db_node_type: String DEFAULT "",              // "HLX-40" | "HLX-80" | etc.
    // NOTE: storage_used_gb is NOT stored - fetched from AWS S3 API
    // NOTE: status is NOT stored - fetched from Kubernetes control plane (TODO)
    created_at: Date DEFAULT NOW,
    updated_at: Date DEFAULT NOW,
}



N::RailwayCluster {
    UNIQUE INDEX railway_project_id: String,
    cluster_name: String,
    build_mode: String DEFAULT "dev",
    railway_service_id: String DEFAULT "",
    railway_environment_id: String DEFAULT "",
    created_at: Date DEFAULT NOW,
    updated_at: Date DEFAULT NOW,
}

N::RailwayInstance {
    UNIQUE INDEX railway_service_id: String,
    INDEX railway_environment_id: String,
    cpu_cores: U64,
    ram_gb: U64,
    created_at: Date DEFAULT NOW,
    updated_at: Date DEFAULT NOW,
}

N::ApiKey {
    UNIQUE INDEX unkey_key_id: String,
    created_at: Date DEFAULT NOW,
}

E::MemberOf {
    From: User,
    To: Workspace,
    Properties: {
        role: String DEFAULT "member",
        joined_at: Date DEFAULT NOW,
    }
}

E::HasProject {
    From: Workspace,
    To: Project,
}

E::HasRailwayCluster {
    From: Project,
    To: RailwayCluster,
}

E::HasObjectCluster {
    From: Project,
    To: ObjectCluster,
}

E::HasInstance {
    From: RailwayCluster,
    To: RailwayInstance,
}

E::CreatedApiKey {
    From: User,
    To: ApiKey,
}
