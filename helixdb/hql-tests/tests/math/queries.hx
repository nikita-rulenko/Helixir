// =============================================================================
// Math Computed Expression Tests
// =============================================================================

// -----------------------------------------------------------------------------
// 1. Basic Math with COUNT (Primary Test Case)
// -----------------------------------------------------------------------------

QUERY test_add_counts(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        total_relations: ADD(_::Out<Contains>::COUNT, _::In<Contains>::COUNT)
    }

// -----------------------------------------------------------------------------
// 2. All Math Operations with COUNT
// -----------------------------------------------------------------------------

QUERY test_sub_counts(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        diff_relations: SUB(_::Out<Contains>::COUNT, _::In<Contains>::COUNT)
    }

QUERY test_mul_counts(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        product_relations: MUL(_::Out<Contains>::COUNT, _::In<Contains>::COUNT)
    }

QUERY test_div_counts(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        ratio_relations: DIV(_::Out<Contains>::COUNT, _::In<Contains>::COUNT)
    }

// -----------------------------------------------------------------------------
// 3. Nested Math Operations
// -----------------------------------------------------------------------------

QUERY test_nested_math(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        complex_calc: ADD(MUL(_::Out<Contains>::COUNT, 2), SUB(_::In<Contains>::COUNT, 1))
    }

QUERY test_deeply_nested_math(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        deep_calc: MUL(ADD(_::Out<Contains>::COUNT, _::In<Contains>::COUNT), DIV(_::Out<Contains>::COUNT, 2))
    }

// -----------------------------------------------------------------------------
// 4. Math with Literals
// -----------------------------------------------------------------------------

QUERY test_add_literal(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        count_plus_ten: ADD(_::Out<Contains>::COUNT, 10)
    }

QUERY test_mul_literal(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        count_doubled: MUL(_::Out<Contains>::COUNT, 2)
    }

QUERY test_sub_literal(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        count_minus_five: SUB(_::Out<Contains>::COUNT, 5)
    }

QUERY test_div_literal(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        count_halved: DIV(_::Out<Contains>::COUNT, 2)
    }

// -----------------------------------------------------------------------------
// 5. Collection Iteration with Math
// -----------------------------------------------------------------------------

QUERY test_items_with_computed_fields() =>
    items <- N<Item>
    RETURN items::|item|{
        name: item::{name},
        adjusted_price: MUL(_::{price}, SUB(1, _::{discount}))
    }

QUERY test_items_with_quantity_calc() =>
    items <- N<Item>
    RETURN items::|item|{
        name: item::{name},
        total_value: MUL(item::{price}, item::{quantity})
    }

// -----------------------------------------------------------------------------
// 6. Math with Edge Traversal Properties
// -----------------------------------------------------------------------------

QUERY test_edge_property_math(container_id: ID) =>
    container <- N<Container>(container_id)
    items <- container::Out<Contains>
    RETURN items::|item|{
        name: item::{name},
        adjusted_position: ADD(item::{position}, 1)
    }
    
// -----------------------------------------------------------------------------
// 8. Multiple Math Expressions in One Return
// -----------------------------------------------------------------------------

QUERY test_multiple_math_fields(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        out_count: _::Out<Contains>::COUNT,
        total: ADD(_::Out<Contains>::COUNT, _::In<Contains>::COUNT),
        difference: SUB(_::Out<Contains>::COUNT, _::In<Contains>::COUNT),
        product: MUL(_::Out<Contains>::COUNT, _::In<Contains>::COUNT)
    }

// -----------------------------------------------------------------------------
// 9. Math with Item-to-Item Relations
// -----------------------------------------------------------------------------

QUERY test_item_relations(item_id: ID) =>
    item <- N<Item>(item_id)
    RETURN item::{
        name,
        outgoing_relations: _::Out<RelatesTo>::COUNT,
        incoming_relations: _::In<RelatesTo>::COUNT,
        total_relations: ADD(_::Out<RelatesTo>::COUNT, _::In<RelatesTo>::COUNT)
    }

// -----------------------------------------------------------------------------
// 10. Complex Nested with Literals and Counts
// -----------------------------------------------------------------------------

QUERY test_complex_formula(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        score: ADD(MUL(_::Out<Contains>::COUNT, 10), MUL(_::In<Contains>::COUNT, 5))
    }

// -----------------------------------------------------------------------------
// 11. POW - Power/Exponentiation
// -----------------------------------------------------------------------------

QUERY test_pow_literal(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        count_squared: POW(_::Out<Contains>::COUNT, 2)
    }

QUERY test_pow_counts(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        power_result: POW(_::Out<Contains>::COUNT, _::In<Contains>::COUNT)
    }

// -----------------------------------------------------------------------------
// 12. MOD - Modulo/Remainder
// -----------------------------------------------------------------------------

QUERY test_mod_literal(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        count_mod_three: MOD(_::Out<Contains>::COUNT, 3)
    }

QUERY test_mod_counts(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        mod_result: MOD(_::Out<Contains>::COUNT, _::In<Contains>::COUNT)
    }

// -----------------------------------------------------------------------------
// 13. ABS - Absolute Value
// -----------------------------------------------------------------------------

QUERY test_abs_difference(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        abs_diff: ABS(SUB(_::Out<Contains>::COUNT, _::In<Contains>::COUNT))
    }

QUERY test_abs_literal() =>
    items <- N<Item>
    RETURN items::|item|{
        name: item::{name},
        abs_adjusted: ABS(SUB(item::{price}, 100))
    }

// -----------------------------------------------------------------------------
// 14. SQRT - Square Root
// -----------------------------------------------------------------------------

QUERY test_sqrt_count(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        sqrt_count: SQRT(_::Out<Contains>::COUNT)
    }

QUERY test_sqrt_literal(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        sqrt_of_sum: SQRT(ADD(_::Out<Contains>::COUNT, 16))
    }

// -----------------------------------------------------------------------------
// 15. MIN - Minimum Value
// -----------------------------------------------------------------------------

QUERY test_min_counts(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        min_relations: MIN(_::Out<Contains>::COUNT, _::In<Contains>::COUNT)
    }

QUERY test_min_with_literal(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        capped_count: MIN(_::Out<Contains>::COUNT, 5)
    }

// -----------------------------------------------------------------------------
// 16. MAX - Maximum Value
// -----------------------------------------------------------------------------

QUERY test_max_counts(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        max_relations: MAX(_::Out<Contains>::COUNT, _::In<Contains>::COUNT)
    }

QUERY test_max_with_literal(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        floor_count: MAX(_::Out<Contains>::COUNT, 1)
    }

// -----------------------------------------------------------------------------
// 17. Combined New Math Operations
// -----------------------------------------------------------------------------

QUERY test_combined_new_ops(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        normalized: DIV(SUB(_::Out<Contains>::COUNT, MIN(_::Out<Contains>::COUNT, _::In<Contains>::COUNT)), MAX(_::Out<Contains>::COUNT, 1))
    }

QUERY test_distance_formula(container_id: ID) =>
    container <- N<Container>(container_id)
    RETURN container::{
        name,
        distance: SQRT(ADD(POW(_::Out<Contains>::COUNT, 2), POW(_::In<Contains>::COUNT, 2)))
    }
