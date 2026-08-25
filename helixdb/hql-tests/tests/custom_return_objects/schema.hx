N::User {
    name: String,
    email: String,
    password: String,
}

N::App {
    name: String,
    description: String DEFAULT "",
    created_at: Date,
}

N::Branch {
    name: String,
}

N::Frontend {
}

N::Backend {
}

N::Element {
    element_id: String,
    name: String,
}

N::PageFolder {
    name: String,
}

N::Page {
    name: String,
}

E::User_Has_App {
    From: User,
    To: App,
    Properties: {
        created_at: Date,
    }
}

E::App_Has_Branch {
    From: App,
    To: Branch,
    Properties: {
        created_at: Date,
    }
}

E::Branch_Has_Frontend {
    From: Branch,
    To: Frontend,
    Properties: {
        created_at: Date,
    }
}

E::Branch_Has_Backend {
    From: Branch,
    To: Backend,
    Properties: {
        created_at: Date,
    }
}

E::Frontend_Contains_PageFolder {
    From: Frontend,
    To: PageFolder,
    Properties: {
        created_at: Date,
        assigned_at: Date,
    }
}

E::Page_Has_Root_Element {
    From: Page,
    To: Element,
    Properties: {
        created_at: Date,
        assigned_at: Date,
    }
}

E::Frontend_Has_Page {
    From: Frontend,
    To: Page,
    Properties: {
        created_at: Date,
        assigned_at: Date,
    }
}

E::PageFolder_Contains_Page {
    From: PageFolder,
    To: Page,
    Properties: {
        created_at: Date,
        assigned_at: Date,
    }
}

E::User_Has_Access_To {
    From: User,
    To: App,
    Properties: {
        created_at: Date,
        assigned_at: Date,
    }
}