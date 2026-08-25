// SHOULD ALL PASS

N::NodeUnusedWithTrailingComma {
    name: String,
    is_admin: Boolean,
}

N::NodeUnusedWithNoTrailingComma {
    name: String,
    is_admin: Boolean
}

E::EdgeUnusedWithNoProps {
    From: NodeUnusedWithTrailingComma,
    To: NodeUnusedWithTrailingComma,
}

E::EdgeUnusedWithoutPropsOrTrailingComma {
    From: NodeUnusedWithTrailingComma,
    To: NodeUnusedWithTrailingComma
}

E::EdgeUnusedWithPropsAndTrailingComma {
    From: NodeUnusedWithTrailingComma,
    To: NodeUnusedWithTrailingComma,
    Properties: {
        prop1: String,
        prop2: String,
    }
}

E::EdgeUnusedWithPropsAndNoTrailingComma {
    From: NodeUnusedWithTrailingComma,
    To: NodeUnusedWithTrailingComma,
    Properties: {
        prop1: String
    }
}
