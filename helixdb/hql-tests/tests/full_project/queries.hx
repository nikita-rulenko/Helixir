// ==================== Auth Queries ====================

QUERY CreateUserGetUserId(github_id: U64, github_login: String, github_name: String, github_email: String, github_avatar: String) =>
    user <- AddN<User>({
        github_id: github_id,
        github_login: github_login,
        github_name: github_name,
        github_email: github_email
    })
    workspace <- AddN<Workspace>({
        name: "Personal",
        url_slug: github_login,
        workspace_type: "personal",
        icon: github_avatar
    })
    AddE<MemberOf>({ role: "owner" })::From(user)::To(workspace)
    RETURN user::ID

QUERY ExistsUserByGithubId(github_id: U64) =>
    user_exists <- EXISTS(N<User>({ github_id: github_id }))
    RETURN user_exists

QUERY GetAllUsers() =>
    users <- N<User>
    RETURN users

QUERY UserIdByGithubId(github_id: U64) =>
    user_id <- N<User>({ github_id: github_id })
    RETURN user_id::ID

QUERY StoreApiKeyRef(user_id: ID, unkey_key_id: String) =>
    user <- N<User>(user_id)
    api_key <- AddN<ApiKey>({ unkey_key_id: unkey_key_id })
    AddE<CreatedApiKey>::From(user)::To(api_key)
    RETURN NONE

// TODO: Needs new name to represent checking for permissions for cluster
//QUERY HasCreatedRailwayCluster(user_id: ID, cluster_id: ID) =>
//    user <- N<User>(user_id)
//    created_cluster <- EXISTS(user::Out<MemberOf>::Out<HasProject>::Out<HasRailwayCluster>::WHERE(_::{id}::EQ(cluster_id)))
//    RETURN created_cluster

//QUERY CreateRailwayCluster(user_id: ID, railway_project_id: String, project_name: String, railway_region: String) =>
//    user <- N<User>(user_id)
//    cluster_id <- AddN<RailwayCluster>({
//        railway_project_id: railway_project_id,
//        project_name: project_name,
//        railway_region: railway_region
//    }
//    RETURN cluster_id::ID

//QUERY GetRailwayCluster(cluster_id: ID) =>
//    cluster <- N<RailwayCluster>(cluster_id)
//    RETURN cluster::{railway_project_id, project_name, railway_region, db_url}

//QUERY RailwayClusterHasInstance(cluster_id: ID) =>
//    cluster <- N<RailwayCluster>(cluster_id)
//    has_instance <- EXISTS(cluster::Out<HasInstance>)
//    RETURN has_instance

//QUERY GetRailwayClusterInstances(cluster_id: ID) =>
//    cluster <- N<RailwayCluster>(cluster_id)
//    instances <- cluster::Out<HasInstance>
//    RETURN instances::{id, railway_service_id, railway_environment_id}

//QUERY CreateInstanceForRailwayCluster(cluster_id: ID, railway_service_id: String, railway_environment_id: String, instance_type: String, storage_gb: U64, ram_gb: U64) =>
//    cluster <- N<RailwayCluster>(cluster_id)
//    instance_id <- AddN<Instance>({
//        railway_service_id: railway_service_id,
//        railway_environment_id: railway_environment_id,
//        instance_type: instance_type,
//        storage_gb: storage_gb,
//        ram_gb: ram_gb,
//    })
//    AddE<HasInstance>::From(cluster)::To(instance_id)
//    RETURN instance_id::ID

//QUERY UpdateRailwayCluster(cluster_id: ID, db_url: String) =>
//    updated <- N<RailwayCluster>(cluster_id)::UPDATE({ db_url: db_url })
//    RETURN NONE


QUERY GetUserById(user_id: ID) =>
    user <- N<User>(user_id)
    RETURN user

QUERY UpdateUsername(user_id: ID, github_name: String, timestamp: Date) =>
    user <- N<User>(user_id)::UPDATE({ github_name: github_name, updated_at: timestamp })
    RETURN user
// ==================== Workspace Queries ====================

QUERY ExistsWorkspaceBySlug(url_slug: String) =>
    exists <- EXISTS(N<Workspace>({ url_slug: url_slug }))
    RETURN exists

QUERY GetUserWorkspaces(user_id: ID) =>
    user <- N<User>(user_id)
    workspace_edges <- user::OutE<MemberOf>

    RETURN workspace_edges::{
        workspace: _::ToN,
        role: _::{role},
        role_id: _::{id}
    }

QUERY CreateWorkspace(user_id: ID, name: String, url_slug: String, plan: String) =>
    workspace <- AddN<Workspace>({
        name: name,
        workspace_type: "organization",
        plan: plan,
        url_slug: url_slug
    })
    RETURN workspace::ID

QUERY CreateWorkspaceOwner(user_id: ID, workspace_id: ID) =>
    user <- N<User>(user_id)
    workspace <- N<Workspace>(workspace_id)
    AddE<MemberOf>({ role: "owner" })::From(user)::To(workspace)
    RETURN NONE

QUERY IsNewWorkspace(workspace_id: ID) =>
    is_new <- EXISTS(N<Workspace>(workspace_id)::InE<MemberOf>)
    RETURN is_new

QUERY ChangeWorkspacePlan(workspace_id: ID, plan: String) =>
    workspace <- N<Workspace>(workspace_id)::UPDATE({ plan: plan })
    RETURN workspace

QUERY UrlSlugSearch(url_slug: String) =>
    url_slug_exists <- EXISTS(N<Workspace>::WHERE(_::{url_slug}::EQ(url_slug)))
    RETURN url_slug_exists



QUERY GetWorkspace(workspace_id: ID) =>
    workspace <- N<Workspace>(workspace_id)
    RETURN workspace

QUERY UpdateWorkspace(workspace_id: ID, url_slug: String, name: String, icon: String, timestamp: Date) =>
    workspace <- N<Workspace>(workspace_id)::UPDATE({ name: name, url_slug: url_slug, icon: icon, updated_at: timestamp })
    RETURN workspace

QUERY UpdateWorkspaceIcon(workspace_id: ID, icon: String, timestamp: Date) =>
    workspace <- N<Workspace>(workspace_id)::UPDATE({ icon: icon, updated_at: timestamp })
    RETURN workspace

QUERY DeleteWorkspace(workspace_id: ID) =>
    DROP N<Workspace>(workspace_id)::Out<HasProject>::Out<HasRailwayCluster>
    DROP N<Workspace>(workspace_id)::Out<HasProject>::Out<HasObjectCluster>
    DROP N<Workspace>(workspace_id)::Out<HasProject>
    DROP N<Workspace>(workspace_id)
    RETURN NONE

QUERY GetWorkspaceMembers(workspace_id: ID) =>
    workspace <- N<Workspace>(workspace_id)
    members <- workspace::InE<MemberOf>
    RETURN members::{
        user: _::FromN,
        role: _::{role},
        role_id: _::{id}
    }

QUERY AddWorkspaceMember(workspace_id: ID, user_id: ID, role: String) =>
    workspace <- N<Workspace>(workspace_id)
    user <- N<User>(user_id)
    AddE<MemberOf>({ role: role })::From(user)::To(workspace)
    RETURN NONE

QUERY UpdateWorkspaceMemberRole(role_id: ID, role: String) =>
    foo <- E<MemberOf>(role_id)::UPDATE({ role: role })
    RETURN NONE

QUERY RemoveWorkspaceMember(role_id: ID) =>
    DROP E<MemberOf>(role_id)
    RETURN NONE

QUERY GetWorkspaceProjects(workspace_id: ID) =>
    workspace <- N<Workspace>(workspace_id)
    projects <- workspace::Out<HasProject>
    RETURN projects::{
        num_clusters: ADD(_::Out<HasRailwayCluster>::COUNT, _::Out<HasObjectCluster>::COUNT),
        ..
    }

QUERY CreateProject(workspace_id: ID, name: String) =>
    workspace <- N<Workspace>(workspace_id)
    project <- AddN<Project>({ name: name })
    AddE<HasProject>::From(workspace)::To(project)
    RETURN project::ID

QUERY GetProject(project_id: ID) =>
    project <- N<Project>(project_id)
    workspace <- project::In<HasProject>::FIRST
    RETURN project::{
        id,
        name,
        railway_clusters: _::Out<HasRailwayCluster>::{
            project_id: project::ID,
            workspace_id: workspace::ID,
            ..
        },
        object_clusters: _::Out<HasObjectCluster>::{
            project_id: project::ID,
            workspace_id: workspace::ID,
            ..
        }
    }

QUERY UserHasProjectAccess(user_id: ID, project_id: ID) =>
    user <- N<User>(user_id)
    has_access <- EXISTS(user::Out<MemberOf>::Out<HasProject>::WHERE(_::{id}::EQ(project_id)))
    RETURN has_access

QUERY UserHasClusterAccess(user_id: ID, cluster_id: ID) =>
    user <- N<User>(user_id)
    has_access <- EXISTS(
                    user::Out<MemberOf>::WHERE(
                        OR(
                            EXISTS(_::Out<HasProject>::Out<HasRailwayCluster>::WHERE(_::{id}::EQ(cluster_id))),
                            EXISTS(_::Out<HasProject>::Out<HasObjectCluster>::WHERE(_::{id}::EQ(cluster_id)))
                        )
                    )
                )
    RETURN has_access

QUERY UpdateProject(project_id: ID, name: String, timestamp: Date) =>
    project <- N<Project>(project_id)::UPDATE({ name: name, updated_at: timestamp })
    RETURN project

QUERY DeleteProject(project_id: ID) =>
    DROP N<Project>(project_id)::Out<HasRailwayCluster>::Out<HasInstance>
    DROP N<Project>(project_id)::Out<HasRailwayCluster>
    DROP N<Project>(project_id)::Out<HasObjectCluster>
    DROP N<Project>(project_id)
    RETURN NONE

QUERY GetProjectClusters(project_id: ID) =>
    project <- N<Project>(project_id)
    railway_clusters <- project::Out<HasRailwayCluster>
    object_clusters <- project::Out<HasObjectCluster>
    RETURN railway_clusters, object_clusters

QUERY CreateRailwayClustersInProject(project_id: ID, clusters: [{railway_project_id: String, cluster_name: String, build_mode: String}]) =>
    project <- N<Project>(project_id)
    FOR {railway_project_id, cluster_name, build_mode} IN clusters {
        cluster <- AddN<RailwayCluster>({ railway_project_id: railway_project_id, cluster_name: cluster_name, build_mode: build_mode })
        AddE<HasRailwayCluster>::From(project)::To(cluster)
    }
    final_clusters <- project::Out<HasRailwayCluster>
    RETURN final_clusters

QUERY GetRailwayCluster(cluster_id: ID) =>
    cluster <- N<RailwayCluster>(cluster_id)
    project <- cluster::In<HasRailwayCluster>::FIRST
    workspace <- project::In<HasProject>::FIRST
    RETURN cluster::{
        project_id: project::ID,
        workspace_id: workspace::ID,
        ..
    }

QUERY UpdateRailwayCluster(cluster_id: ID, cluster_name: String, build_mode: String, timestamp: Date) =>
    cluster <- N<RailwayCluster>(cluster_id)::UPDATE({ cluster_name: cluster_name, build_mode: build_mode, updated_at: timestamp })
    RETURN cluster

QUERY UpdateRailwayClusterServiceInfo(cluster_id: ID, railway_service_id: String, railway_environment_id: String, timestamp: Date) =>
    cluster <- N<RailwayCluster>(cluster_id)::UPDATE({
        railway_service_id: railway_service_id,
        railway_environment_id: railway_environment_id,
        updated_at: timestamp
    })
    RETURN cluster

QUERY DeleteRailwayCluster(cluster_id: ID) =>
    DROP N<RailwayCluster>(cluster_id)::Out<HasInstance>
    DROP N<RailwayCluster>(cluster_id)
    RETURN NONE

QUERY GetUserApiTokens(user_id: ID) =>
      user <- N<User>(user_id)
      tokens <- user::Out<CreatedApiKey>
      RETURN tokens


QUERY DeleteApiToken(token_id: ID) =>
      DROP N<ApiKey>(token_id)
      RETURN NONE

QUERY GetUserByGithubLogin(github_login: String) =>
      user <- N<User>::WHERE(_::{github_login}::EQ(github_login))
      RETURN user

  QUERY GetUserByGithubEmail(github_email: String) =>
      user <- N<User>::WHERE(_::{github_email}::EQ(github_email))
      RETURN user

QUERY GetWorkspaceFromRailwayCluster(cluster_id: ID) =>
    workspace <- N<RailwayCluster>::In<HasRailwayCluster>::In<HasProject>::FIRST
    RETURN workspace

QUERY GetWorkspaceFromObjectCluster(cluster_id: ID) =>
    workspace <- N<ObjectCluster>::In<HasObjectCluster>::In<HasProject>::FIRST
    RETURN workspace

// ==================== Object Cluster Queries ====================

QUERY CreateObjectCluster(
    project_id: ID,
    cluster_name: String,
    availability_mode: String,
    min_instances: U64,
    max_instances: U64,
    gateway_node_type: String,
    db_node_type: String
) =>
    project <- N<Project>(project_id)
    cluster <- AddN<ObjectCluster>({
        cluster_name: cluster_name,
        availability_mode: availability_mode,
        min_instances: min_instances,
        max_instances: max_instances,
        gateway_node_type: gateway_node_type,
        db_node_type: db_node_type
    })
    AddE<HasObjectCluster>::From(project)::To(cluster)
    RETURN cluster::ID

QUERY GetObjectCluster(cluster_id: ID) =>
    cluster <- N<ObjectCluster>(cluster_id)
    project <- cluster::In<HasObjectCluster>::FIRST
    workspace <- project::In<HasProject>::FIRST
    RETURN cluster::{
        project_id: project::ID,
        workspace_id: workspace::ID,
        ..
    }

QUERY UpdateObjectClusterInstances(
    cluster_id: ID,
    min_instances: U64,
    max_instances: U64,
    updated_at: Date
) =>
    cluster <- N<ObjectCluster>(cluster_id)::UPDATE({
        min_instances: min_instances,
        max_instances: max_instances,
        updated_at: updated_at
    })
    RETURN cluster

QUERY DeleteObjectCluster(cluster_id: ID) =>
    DROP N<ObjectCluster>(cluster_id)
    RETURN NONE
