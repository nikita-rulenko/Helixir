

use chrono::{DateTime, Utc};


pub fn cosine_similarity(vec1: &[f32], vec2: &[f32]) -> f64 {
    if vec1.is_empty() || vec2.is_empty() || vec1.len() != vec2.len() {
        return 0.0;
    }

    let dot_product: f32 = vec1.iter().zip(vec2.iter()).map(|(a, b)| a * b).sum();
    let mag1: f32 = vec1.iter().map(|a| a * a).sum::<f32>().sqrt();
    let mag2: f32 = vec2.iter().map(|b| b * b).sum::<f32>().sqrt();

    if mag1 == 0.0 || mag2 == 0.0 {
        return 0.0;
    }

    let similarity = f64::from(dot_product / (mag1 * mag2));
    
    ((similarity + 1.0) / 2.0).clamp(0.0, 1.0)
}


pub fn calculate_temporal_freshness(created_at: &str, decay_days: f64) -> f64 {
    let created = match DateTime::parse_from_rfc3339(created_at) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => {
            
            if let Ok(dt) = created_at.replace('Z', "+00:00").parse::<DateTime<Utc>>() {
                dt
            } else {
                return 0.5; 
            }
        }
    };

    let now = Utc::now();
    let duration = now.signed_duration_since(created);
    let days_old = duration.num_seconds() as f64 / 86400.0;

    
    let freshness = (-days_old / decay_days).exp();
    freshness.clamp(0.0, 1.0)
}


pub fn calculate_vector_combined_score(vector_score: f64, temporal_score: f64) -> f64 {
    (vector_score * 0.7 + temporal_score * 0.3).clamp(0.0, 1.0)
}


pub fn calculate_graph_combined_score(
    semantic_sim: f64,
    graph_score: f64,
    temporal_score: f64,
) -> f64 {
    (semantic_sim * 0.3 + graph_score * 0.5 + temporal_score * 0.2).clamp(0.0, 1.0)
}


pub fn calculate_graph_score(edge_weight: f64, parent_score: f64) -> f64 {
    (edge_weight * parent_score).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let vec1 = vec![1.0, 0.0, 0.0];
        let vec2 = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&vec1, &vec2);
        assert!((sim - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let vec1 = vec![1.0, 0.0, 0.0];
        let vec2 = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&vec1, &vec2);
        assert!((sim - 0.5).abs() < 0.01); 
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let vec1 = vec![1.0, 0.0, 0.0];
        let vec2 = vec![-1.0, 0.0, 0.0];
        let sim = cosine_similarity(&vec1, &vec2);
        assert!((sim - 0.0).abs() < 0.01); 
    }

    #[test]
    fn test_temporal_freshness_now() {
        let now = Utc::now().to_rfc3339();
        let freshness = calculate_temporal_freshness(&now, 30.0);
        assert!(freshness > 0.99);
    }

    #[test]
    fn test_temporal_freshness_old() {
        
        let old = (Utc::now() - chrono::Duration::days(90)).to_rfc3339();
        let freshness = calculate_temporal_freshness(&old, 30.0);
        
        assert!(freshness < 0.1);
    }

    #[test]
    fn test_combined_scores() {
        let vector_combined = calculate_vector_combined_score(0.8, 0.9);
        assert!((vector_combined - 0.83).abs() < 0.01);

        let graph_combined = calculate_graph_combined_score(0.5, 0.8, 0.9);
        
        assert!((graph_combined - 0.73).abs() < 0.01);
    }
}

