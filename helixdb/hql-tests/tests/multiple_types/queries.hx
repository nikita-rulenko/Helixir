N::File2 {
    name: String,
    is_admin: Boolean,
    f1: I8,
    f2: I16,
    f3: I32,
    f4: I64,
    f5: F32,
    f6: F64,
    f7: String,
    f8: U8,
    f9: U16,
    f10: U32,
    f11: U64,
    f12: U128,
}

E::EdgeFile2 {
    From: File2,
    To: File2,
    Properties: {
        name: String,
        is_admin: Boolean,
        f1: I8,
        f2: I16,
        f3: I32,
        f4: I64,
        f5: F32,
        f6: F64,
        f7: String,
        f8: U8,
        f9: U16,
        f10: U32,
        f11: U64,
        f12: U128,
    }
}


QUERY file2(name: String) =>
    // Should pass
    user <- AddN<File2>({name: name, is_admin: true, f1: 1, f2: 2, f3: 3, f4: 4, f5: 5.0, f6: 6.0, f7: "7", f8: 8, f9: 9, f10: 10, f11: 11, f12: 12})
    user2 <- AddN<File2>({name: name, is_admin: true, f1: 1, f2: 2, f3: 3, f4: 4, f5: 5.0, f6: 6.5, f7: "7", f8: 8, f9: 9, f10: 10, f11: 11, f12: 12})
    AddE<EdgeFile2>({name: name, is_admin: true, f1: 1, f2: 2, f3: 3, f4: 4, f5: 5.3, f6: 6.0, f7: "7", f8: 8, f9: 9, f10: 10, f11: 11, f12: 12})::From(user)::To(user2)
    RETURN user
