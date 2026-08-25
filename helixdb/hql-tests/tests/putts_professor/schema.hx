// NODES //

N::Professor {
    name: String,
    title: String,
    page: String,
    bio: String,
}

// We have this node so that we can link professors by research area + description
N::ResearchArea {
    research_area: String,
}

N::Department {
    name: String,
}   

N::University {
    name: String,
}

N::Lab {
    name: String,
    research_focus: String,
}


// Connect Professor to Lab
E::HasLab {
    From: Professor,
    To: Lab,
}

// Connect Professor to Research Area
E::HasResearchArea {
    From: Professor,
    To: ResearchArea,
}

// EDGES //

// Connect Professor to University
E::HasUniversity {
    From: Professor,
    To: University,
    Properties: {
        since: Date DEFAULT NOW,
    }
}

// Connect Professor to Department
E::HasDepartment {
    From: Professor,
    To: Department,
    Properties: {
        since: Date DEFAULT NOW,
    }
}

// VECTORS //


// Connect Professor to Research Area + Description
V::ResearchAreaAndDescriptionEmbedding {
    areas_and_descriptions: String,
}

E::HasResearchAreaAndDescriptionEmbedding {
    From: Professor,
    To: ResearchAreaAndDescriptionEmbedding,
    Properties: {
        areas_and_descriptions: String,
    }
}