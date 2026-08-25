// DEFAULT CODE
// use helix_db::helix_engine::traversal_core::config::Config;

// pub fn config() -> Option<Config> {
//     None
// }

use bumpalo::Bump;
use chrono::{DateTime, Utc};
use heed3::RoTxn;
use helix_db::{
    embed, embed_async, field_addition_from_old_field, field_addition_from_value, field_type_cast,
    helix_engine::{
        reranker::{
            RerankAdapter,
            fusion::{DistanceMethod, MMRReranker, RRFReranker},
        },
        traversal_core::{
            config::{Config, GraphConfig, VectorConfig},
            ops::{
                bm25::search_bm25::SearchBM25Adapter,
                g::G,
                in_::{in_::InAdapter, in_e::InEdgesAdapter, to_n::ToNAdapter, to_v::ToVAdapter},
                out::{
                    from_n::FromNAdapter, from_v::FromVAdapter, out::OutAdapter,
                    out_e::OutEdgesAdapter,
                },
                source::{
                    add_e::AddEAdapter, add_n::AddNAdapter, e_from_id::EFromIdAdapter,
                    e_from_type::EFromTypeAdapter, n_from_id::NFromIdAdapter,
                    n_from_index::NFromIndexAdapter, n_from_type::NFromTypeAdapter,
                    v_from_id::VFromIdAdapter, v_from_type::VFromTypeAdapter,
                },
                util::{
                    aggregate::AggregateAdapter,
                    count::CountAdapter,
                    dedup::DedupAdapter,
                    drop::Drop,
                    exist::Exist,
                    filter_mut::FilterMut,
                    filter_ref::FilterRefAdapter,
                    group_by::GroupByAdapter,
                    map::MapAdapter,
                    order::OrderByAdapter,
                    paths::{PathAlgorithm, ShortestPathAdapter},
                    range::RangeAdapter,
                    update::UpdateAdapter,
                },
                vectors::{
                    brute_force_search::BruteForceSearchVAdapter, insert::InsertVAdapter,
                    search::SearchVAdapter,
                },
            },
            traversal_value::TraversalValue,
        },
        types::GraphError,
        vector_core::vector::HVector,
    },
    helix_gateway::{
        embedding_providers::{EmbeddingModel, get_embedding_model},
        mcp::mcp::{MCPHandler, MCPHandlerSubmission, MCPToolInput},
        router::router::{HandlerInput, IoContFn},
    },
    node_matches, props,
    protocol::{
        format::Format,
        response::Response,
        value::{
            Value,
            casting::{CastType, cast},
        },
    },
    utils::{
        id::{ID, uuid_str},
        items::{Edge, Node},
        properties::ImmutablePropertiesMap,
    },
};
use helix_macros::{handler, mcp_handler, migration, tool_call};
use sonic_rs::{Deserialize, Serialize, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

// Re-export scalar types for generated code
type I8 = i8;
type I16 = i16;
type I32 = i32;
type I64 = i64;
type U8 = u8;
type U16 = u16;
type U32 = u32;
type U64 = u64;
type U128 = u128;
type F32 = f32;
type F64 = f64;

pub fn config() -> Option<Config> {
    return Some(Config {
        vector_config: Some(VectorConfig {
            m: Some(16),
            ef_construction: Some(128),
            ef_search: Some(768),
        }),
        graph_config: Some(GraphConfig {
            secondary_indices: Some(vec![
                "github_id".to_string(),
                "railway_project_id".to_string(),
            ]),
        }),
        db_max_size_gb: Some(10),
        mcp: Some(true),
        bm25: Some(true),
        schema: Some(
            r#"{
  "schema": {
    "nodes": [
      {
        "name": "ApiKey",
        "properties": {
          "id": "ID",
          "label": "String",
          "unkey_key_id": "String"
        }
      },
      {
        "name": "User",
        "properties": {
          "updated_at": "Date",
          "github_email": "String",
          "github_id": "U64",
          "created_at": "Date",
          "id": "ID",
          "github_name": "String",
          "github_login": "String",
          "label": "String"
        }
      },
      {
        "name": "Cluster",
        "properties": {
          "created_at": "Date",
          "label": "String",
          "railway_region": "String",
          "updated_at": "Date",
          "project_name": "String",
          "railway_project_id": "String",
          "db_url": "String",
          "id": "ID"
        }
      },
      {
        "name": "Instance",
        "properties": {
          "ram_gb": "U64",
          "railway_environment_id": "String",
          "instance_type": "String",
          "storage_gb": "U64",
          "label": "String",
          "updated_at": "Date",
          "railway_service_id": "String",
          "created_at": "Date",
          "id": "ID"
        }
      }
    ],
    "vectors": [],
    "edges": [
      {
        "name": "HasInstance",
        "from": "Cluster",
        "to": "Instance",
        "properties": {}
      },
      {
        "name": "CreatedApiKey",
        "from": "User",
        "to": "ApiKey",
        "properties": {}
      },
      {
        "name": "CreatedCluster",
        "from": "User",
        "to": "Cluster",
        "properties": {}
      }
    ]
  },
  "queries": [
    {
      "name": "CreateCluster",
      "parameters": {
        "user_id": "ID",
        "railway_region": "String",
        "project_name": "String",
        "railway_project_id": "String"
      },
      "returns": []
    },
    {
      "name": "CreateUserGetUserId",
      "parameters": {
        "github_id": "U64",
        "github_login": "String",
        "github_name": "String",
        "github_email": "String"
      },
      "returns": []
    },
    {
      "name": "HasCreatedCluster",
      "parameters": {
        "user_id": "ID",
        "cluster_id": "ID"
      },
      "returns": [
        "created_cluster"
      ]
    },
    {
      "name": "ClusterHasInstance",
      "parameters": {
        "cluster_id": "ID"
      },
      "returns": [
        "has_instance"
      ]
    },
    {
      "name": "UpdateCluster",
      "parameters": {
        "db_url": "String",
        "cluster_id": "ID"
      },
      "returns": []
    },
    {
      "name": "GetCluster",
      "parameters": {
        "cluster_id": "ID"
      },
      "returns": []
    },
    {
      "name": "UserIdByGithubId",
      "parameters": {
        "github_id": "U64"
      },
      "returns": []
    },
    {
      "name": "ExistsUserByGithubId",
      "parameters": {
        "github_id": "U64"
      },
      "returns": [
        "user_exists"
      ]
    },
    {
      "name": "StoreApiKeyRef",
      "parameters": {
        "user_id": "ID",
        "unkey_key_id": "String"
      },
      "returns": []
    },
    {
      "name": "CreateInstanceForCluster",
      "parameters": {
        "storage_gb": "U64",
        "instance_type": "String",
        "ram_gb": "U64",
        "railway_environment_id": "String",
        "cluster_id": "ID",
        "railway_service_id": "String"
      },
      "returns": []
    },
    {
      "name": "GetClusterInstances",
      "parameters": {
        "cluster_id": "ID"
      },
      "returns": []
    }
  ]
}"#
            .to_string(),
        ),
        embedding_model: Some("text-embedding-ada-002".to_string()),
        graphvis_node_label: None,
    });
}

pub struct User {
    pub github_id: u64,
    pub github_login: String,
    pub github_name: String,
    pub github_email: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct Cluster {
    pub railway_project_id: String,
    pub project_name: String,
    pub railway_region: String,
    pub db_url: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct Instance {
    pub railway_service_id: String,
    pub railway_environment_id: String,
    pub instance_type: String,
    pub storage_gb: u64,
    pub ram_gb: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct ApiKey {
    pub unkey_key_id: String,
}

pub struct CreatedCluster {
    pub from: User,
    pub to: Cluster,
}

pub struct HasInstance {
    pub from: Cluster,
    pub to: Instance,
}

pub struct CreatedApiKey {
    pub from: User,
    pub to: ApiKey,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CreateClusterInput {
    pub user_id: ID,
    pub railway_project_id: String,
    pub project_name: String,
    pub railway_region: String,
}
#[handler]
pub fn CreateCluster(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input
        .request
        .in_fmt
        .deserialize::<CreateClusterInput>(&input.request.body)?;
    let arena = Bump::new();
    let mut txn = db
        .graph_env
        .write_txn()
        .map_err(|e| GraphError::New(format!("Failed to start write transaction: {:?}", e)))?;
    let user = G::new(&db, &txn, &arena)
        .n_from_id(&data.user_id)
        .collect_to_obj()?;
    let cluster_id = G::new_mut(&db, &arena, &mut txn)
        .add_n(
            "Cluster",
            Some(ImmutablePropertiesMap::new(
                6,
                vec![
                    ("db_url", Value::from("")),
                    ("project_name", Value::from(&data.project_name)),
                    ("updated_at", Value::from(chrono::Utc::now().to_rfc3339())),
                    ("created_at", Value::from(chrono::Utc::now().to_rfc3339())),
                    ("railway_project_id", Value::from(&data.railway_project_id)),
                    ("railway_region", Value::from(&data.railway_region)),
                ]
                .into_iter(),
                &arena,
            )),
            Some(&["railway_project_id"]),
        )
        .collect_to_obj()?;
    G::new_mut(&db, &arena, &mut txn)
        .add_edge("CreatedCluster", None, user.id(), cluster_id.id(), false)
        .collect_to_obj()?;
    let response = json!({
        "cluster_id": uuid_str(cluster_id.id(), &arena)
    });
    txn.commit()
        .map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
    Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CreateUserGetUserIdInput {
    pub github_id: u64,
    pub github_login: String,
    pub github_name: String,
    pub github_email: String,
}
#[handler]
pub fn CreateUserGetUserId(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input
        .request
        .in_fmt
        .deserialize::<CreateUserGetUserIdInput>(&input.request.body)?;
    let arena = Bump::new();
    let mut txn = db
        .graph_env
        .write_txn()
        .map_err(|e| GraphError::New(format!("Failed to start write transaction: {:?}", e)))?;
    let user_id = G::new_mut(&db, &arena, &mut txn)
        .add_n(
            "User",
            Some(ImmutablePropertiesMap::new(
                6,
                vec![
                    ("github_name", Value::from(&data.github_name)),
                    ("github_id", Value::from(&data.github_id)),
                    ("github_login", Value::from(&data.github_login)),
                    ("github_email", Value::from(&data.github_email)),
                    ("created_at", Value::from(chrono::Utc::now().to_rfc3339())),
                    ("updated_at", Value::from(chrono::Utc::now().to_rfc3339())),
                ]
                .into_iter(),
                &arena,
            )),
            Some(&["github_id"]),
        )
        .collect_to_obj()?;
    let response = json!({
        "user_id": uuid_str(user_id.id(), &arena)
    });
    txn.commit()
        .map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
    Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct HasCreatedClusterInput {
    pub user_id: ID,
    pub cluster_id: ID,
}
#[handler]
pub fn HasCreatedCluster(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input
        .request
        .in_fmt
        .deserialize::<HasCreatedClusterInput>(&input.request.body)?;
    let arena = Bump::new();
    let txn = db
        .graph_env
        .read_txn()
        .map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let user = G::new(&db, &txn, &arena)
        .n_from_id(&data.user_id)
        .collect_to_obj()?;
    let created_cluster = Exist::exists(
        &mut G::from_iter(&db, &txn, std::iter::once(user.clone()), &arena)
            .out_node("CreatedCluster")
            .filter_ref(|val, txn| {
                if let Ok(val) = val {
                    Ok(Value::Id(ID::from(val.id())) == data.cluster_id.clone())
                } else {
                    Ok(false)
                }
            }),
    );
    let response = json!({
        "created_cluster": created_cluster
    });
    txn.commit()
        .map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
    Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ClusterHasInstanceInput {
    pub cluster_id: ID,
}
#[handler]
pub fn ClusterHasInstance(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input
        .request
        .in_fmt
        .deserialize::<ClusterHasInstanceInput>(&input.request.body)?;
    let arena = Bump::new();
    let txn = db
        .graph_env
        .read_txn()
        .map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let cluster = G::new(&db, &txn, &arena)
        .n_from_id(&data.cluster_id)
        .collect_to_obj()?;
    let has_instance = Exist::exists(
        &mut G::from_iter(&db, &txn, std::iter::once(cluster.clone()), &arena)
            .out_node("HasInstance"),
    );
    let response = json!({
        "has_instance": has_instance
    });
    txn.commit()
        .map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
    Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UpdateClusterInput {
    pub cluster_id: ID,
    pub db_url: String,
}
#[handler]
pub fn UpdateCluster(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input
        .request
        .in_fmt
        .deserialize::<UpdateClusterInput>(&input.request.body)?;
    let arena = Bump::new();
    let mut txn = db
        .graph_env
        .write_txn()
        .map_err(|e| GraphError::New(format!("Failed to start write transaction: {:?}", e)))?;
    let updated = {
        let update_tr = G::new(&db, &txn, &arena)
            .n_from_id(&data.cluster_id)
            .collect::<Result<Vec<_>, _>>()?;
        G::new_mut_from_iter(&db, &mut txn, update_tr.iter().cloned(), &arena)
            .update(&[("db_url", Value::from(&data.db_url))])
            .collect_to_obj()?
    };
    txn.commit()
        .map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
    Ok(input.request.out_fmt.create_response(&()))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GetClusterInput {
    pub cluster_id: ID,
}
#[derive(Serialize)]
pub struct GetClusterClusterReturnType<'a> {
    pub railway_project_id: Option<&'a Value>,
    pub project_name: Option<&'a Value>,
    pub railway_region: Option<&'a Value>,
    pub db_url: Option<&'a Value>,
}

#[handler]
pub fn GetCluster(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input
        .request
        .in_fmt
        .deserialize::<GetClusterInput>(&input.request.body)?;
    let arena = Bump::new();
    let txn = db
        .graph_env
        .read_txn()
        .map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let cluster = G::new(&db, &txn, &arena)
        .n_from_id(&data.cluster_id)
        .collect_to_obj()?;
    let response = json!({
        "cluster": GetClusterClusterReturnType {
            railway_project_id: cluster.get_property("railway_project_id"),
            project_name: cluster.get_property("project_name"),
            railway_region: cluster.get_property("railway_region"),
            db_url: cluster.get_property("db_url"),
        }
    });
    txn.commit()
        .map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
    Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UserIdByGithubIdInput {
    pub github_id: u64,
}
#[handler]
pub fn UserIdByGithubId(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input
        .request
        .in_fmt
        .deserialize::<UserIdByGithubIdInput>(&input.request.body)?;
    let arena = Bump::new();
    let txn = db
        .graph_env
        .read_txn()
        .map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let user_id = G::new(&db, &txn, &arena)
        .n_from_index("User", "github_id", &data.github_id)
        .collect_to_obj()?;
    let response = json!({
        "user_id": uuid_str(user_id.id(), &arena)
    });
    txn.commit()
        .map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
    Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ExistsUserByGithubIdInput {
    pub github_id: u64,
}
#[handler]
pub fn ExistsUserByGithubId(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input
        .request
        .in_fmt
        .deserialize::<ExistsUserByGithubIdInput>(&input.request.body)?;
    let arena = Bump::new();
    let txn = db
        .graph_env
        .read_txn()
        .map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let user_exists = Exist::exists(&mut G::new(&db, &txn, &arena).n_from_index(
        "User",
        "github_id",
        &data.github_id,
    ));
    let response = json!({
        "user_exists": user_exists
    });
    txn.commit()
        .map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
    Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct StoreApiKeyRefInput {
    pub user_id: ID,
    pub unkey_key_id: String,
}
#[handler]
pub fn StoreApiKeyRef(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input
        .request
        .in_fmt
        .deserialize::<StoreApiKeyRefInput>(&input.request.body)?;
    let arena = Bump::new();
    let mut txn = db
        .graph_env
        .write_txn()
        .map_err(|e| GraphError::New(format!("Failed to start write transaction: {:?}", e)))?;
    let user = G::new(&db, &txn, &arena)
        .n_from_id(&data.user_id)
        .collect_to_obj()?;
    let api_key = G::new_mut(&db, &arena, &mut txn)
        .add_n(
            "ApiKey",
            Some(ImmutablePropertiesMap::new(
                1,
                vec![("unkey_key_id", Value::from(&data.unkey_key_id))].into_iter(),
                &arena,
            )),
            None,
        )
        .collect_to_obj()?;
    G::new_mut(&db, &arena, &mut txn)
        .add_edge("CreatedApiKey", None, user.id(), api_key.id(), false)
        .collect_to_obj()?;
    txn.commit()
        .map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
    Ok(input.request.out_fmt.create_response(&()))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CreateInstanceForClusterInput {
    pub cluster_id: ID,
    pub railway_service_id: String,
    pub railway_environment_id: String,
    pub instance_type: String,
    pub storage_gb: u64,
    pub ram_gb: u64,
}
#[handler]
pub fn CreateInstanceForCluster(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input
        .request
        .in_fmt
        .deserialize::<CreateInstanceForClusterInput>(&input.request.body)?;
    let arena = Bump::new();
    let mut txn = db
        .graph_env
        .write_txn()
        .map_err(|e| GraphError::New(format!("Failed to start write transaction: {:?}", e)))?;
    let cluster = G::new(&db, &txn, &arena)
        .n_from_id(&data.cluster_id)
        .collect_to_obj()?;
    let instance_id = G::new_mut(&db, &arena, &mut txn)
        .add_n(
            "Instance",
            Some(ImmutablePropertiesMap::new(
                7,
                vec![
                    ("railway_service_id", Value::from(&data.railway_service_id)),
                    ("ram_gb", Value::from(&data.ram_gb)),
                    ("created_at", Value::from(chrono::Utc::now().to_rfc3339())),
                    ("updated_at", Value::from(chrono::Utc::now().to_rfc3339())),
                    ("instance_type", Value::from(&data.instance_type)),
                    ("storage_gb", Value::from(&data.storage_gb)),
                    (
                        "railway_environment_id",
                        Value::from(&data.railway_environment_id),
                    ),
                ]
                .into_iter(),
                &arena,
            )),
            None,
        )
        .collect_to_obj()?;
    G::new_mut(&db, &arena, &mut txn)
        .add_edge("HasInstance", None, cluster.id(), instance_id.id(), false)
        .collect_to_obj()?;
    let response = json!({
        "instance_id": uuid_str(instance_id.id(), &arena)
    });
    txn.commit()
        .map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
    Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GetClusterInstancesInput {
    pub cluster_id: ID,
}
#[derive(Serialize)]
pub struct GetClusterInstancesInstancesReturnType<'a> {
    pub id: &'a str,
    pub railway_service_id: Option<&'a Value>,
    pub railway_environment_id: Option<&'a Value>,
}

#[handler]
pub fn GetClusterInstances(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input
        .request
        .in_fmt
        .deserialize::<GetClusterInstancesInput>(&input.request.body)?;
    let arena = Bump::new();
    let txn = db
        .graph_env
        .read_txn()
        .map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let cluster = G::new(&db, &txn, &arena)
        .n_from_id(&data.cluster_id)
        .collect_to_obj()?;
    let instances = G::from_iter(&db, &txn, std::iter::once(cluster.clone()), &arena)
        .out_node("HasInstance")
        .collect::<Result<Vec<_>, _>>()?;
    let response = json!({
        "instances": instances.iter().map(|instance| GetClusterInstancesInstancesReturnType {
            id: uuid_str(instance.id(), &arena),
            railway_service_id: instance.get_property("railway_service_id"),
            railway_environment_id: instance.get_property("railway_environment_id"),
        }).collect::<Vec<_>>()
    });
    txn.commit()
        .map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
    Ok(input.request.out_fmt.create_response(&response))
}
