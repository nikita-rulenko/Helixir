QUERY CreateUser(gh_id: U64, gh_login: String, name: String, email: String) =>
    user <- AddN<User>({gh_id: gh_id, gh_login: gh_login, name: name, email: email})
    RETURN user

QUERY LookupUser(gh_id: U64) =>
    user <- N<User>({gh_id: gh_id})
    RETURN user


QUERY CreateCluster(user_id: ID, region: String, instance_type: String, storage_gb: I64, ram_gb: I64) =>
    user <- N<User>(user_id)
    new_cluster <- AddN<Cluster>({region: region})
    new_instance <- AddN<Instance>({
        region: region,
        instance_type: instance_type,
        storage_gb: storage_gb,
        ram_gb: ram_gb
    })
    AddE<CreatedCluster>::From(user)::To(new_cluster)
    AddE<CreatedInstance>::From(new_cluster)::To(new_instance)
    RETURN new_cluster

QUERY GetInstancesForUser(user_id: ID) =>
    instances <- N<User>(user_id)::Out<CreatedCluster>::Out<CreatedInstance>
    RETURN instances

QUERY UpdateClusterStatus(cluster_id: ID, status: String) =>
    clusters <- N<Cluster>(cluster_id)::UPDATE({status: status})
    RETURN clusters

QUERY UpdateClusterApiUrl(cluster_id: ID, api_url: String) =>
    clusters <- N<Cluster>(cluster_id)::UPDATE({api_url: api_url})
    RETURN clusters

QUERY GetClusterURL(cluster_id: ID) =>
    clusters <- N<Cluster>(cluster_id)
    RETURN clusters::{api_url}

QUERY ListAllUsersWithClusters() =>
    users <- N<User>
    RETURN users::|user|{
        clusters: user::Out<CreatedCluster>::|cluster|{
            instances: cluster::Out<CreatedInstance>,
            ..
        },
        ..
    }

QUERY VerifyUserAccessToCluster(user_id: ID, cluster_id: ID) =>
    user <- N<User>(user_id)
    cluster <- N<Cluster>(cluster_id)
    can_access <- EXISTS(user::Out<CreatedCluster>::WHERE(_::{id}::EQ(cluster_id)))
    RETURN can_access

