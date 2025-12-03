

pub fn cosine_similarity(vec1: &[f32], vec2: &[f32]) -> f64 {
    if vec1.len() != vec2.len() || vec1.is_empty() {
        return 0.0;
    }

    let dot_product: f32 = vec1.iter().zip(vec2.iter()).map(|(a, b)| a * b).sum();
    let mag1: f32 = vec1.iter().map(|a| a * a).sum::<f32>().sqrt();
    let mag2: f32 = vec2.iter().map(|b| b * b).sum::<f32>().sqrt();

    if mag1 == 0.0 || mag2 == 0.0 {
        return 0.0;
    }

    (dot_product / (mag1 * mag2)) as f64
}


pub fn batch_cosine_similarity(query: &[f32], candidates: &[Vec<f32>]) -> Vec<f64> {
    candidates
        .iter()
        .map(|candidate| cosine_similarity(query, candidate))
        .collect()
}
