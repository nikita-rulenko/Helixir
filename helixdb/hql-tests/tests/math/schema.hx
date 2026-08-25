N::Item {
    name: String,
    price: F64,
    quantity: I32,
    discount: F64,
}

N::Container {
    name: String,
    capacity: I32,
}

E::Contains {
    From: Container,
    To: Item,
    Properties: {
        position: I32,
    }
}

E::RelatesTo {
    From: Item,
    To: Item,
}
