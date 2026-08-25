N::Firm {
    INDEX externalId: String,
    name: String,
    description: String,
    domain: String,
    email: String,
    phone: String,
    city: String,
    state: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
}

N::User {
    INDEX externalId: String,
    firmId: String,
    firstName: String,
    lastName: String,
    email: String,
    phone: String,
    designation: String,
    jobTitle: String,
    category: String,
    accountType: String,
    admin: Boolean DEFAULT false,
    areasOfExpertise: String,
    overview: String,
    yearsOfExperience: I64 DEFAULT 0,
    createdAt: String,
    deletedAt: String DEFAULT "",
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

N::Project {
    INDEX externalId: String,
    firmId: String,
    clientId: String,
    billingClientId: String,
    name: String,
    description: String,
    referenceNumber: String,
    location: String,
    clientName: String,
    projectManagerName: String,
    projectManagerId: String,
    budget: F64 DEFAULT 0.0,
    startDate: String,
    endDate: String,
    status: String,
    entityClassification: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
    suitabilityDecision: String,
    suitabilityConfidenceScore: F64 DEFAULT 0.0,
    suitabilityReason: String,
    suitabilityGeneratedAt: String,
    recommendationDecision: String,
    recommendationConfidenceScore: F64 DEFAULT 0.0,
    pursuitStatus: String,
    pursuitPriority: String,
    pursuitArchived: Boolean DEFAULT false,
    serviceLines: String,
    endMarkets: String,
    opportunitySourceCategory: String,
    opportunitySourceDetails: String,
    responsibility: String,
    opportunityUrl: String,
    opportunityDescription: String,
    estimatedFee: F64 DEFAULT 0.0,
    estimatedConstructionCost: F64 DEFAULT 0.0,
    expectedRevenue: F64 DEFAULT 0.0,
    announcementDate: String,
    rfpIssueDate: String,
    submissionsOpenDate: String,
    questionsDueDate: String,
    bidDate: String,
    interviewDate: String,
    awardAnnouncementDate: String,
    noticeToProceedDate: String,
    expectedStartDate: String,
    expectedEndDate: String,
}

N::Vendor {
    INDEX externalId: String,
    firmId: String,
    name: String,
    description: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
}

N::Client {
    INDEX externalId: String,
    firmId: String,
    name: String,
    referenceNumber: String,
    description: String,
    website: String,
    email: String,
    phone: String,
    status: String,
    clientType: String,
    clientSubType: String,
    market: String,
    governmentAgency: String,
    parentClientId: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

// -----------------------------------------------------------------------------
// FILE SYSTEM
// -----------------------------------------------------------------------------

N::FilesRoot {
    INDEX externalId: String,
    firmId: String,
    name: String,
}

N::Folder {
    INDEX externalId: String,
    firmId: String,
    parentId: String,
    name: String,
    path: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
}

N::File {
    INDEX externalId: String,
    firmId: String,
    folderId: String,
    folderPath: String,
    name: String,
    path: String,
    gcsPath: String,
    mimeType: String,
    sizeBytes: I64 DEFAULT 0,
    projectId: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

// -----------------------------------------------------------------------------
// PROPOSAL STRUCTURE
// -----------------------------------------------------------------------------

N::Proposal {
    INDEX externalId: String,
    projectId: String,
    firmId: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
}

N::Evaluation {
    INDEX externalId: String,
    projectId: String,
    firmId: String,
    sectionsGcsBasePath: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
}

N::EvaluationSection {
    INDEX externalId: String,
    evaluationId: String,
    name: String,
    gcsPath: String,
    projectId: String,
    firmId: String,
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

N::RfxDocuments {
    INDEX externalId: String,
    projectId: String,
    firmId: String,
}

N::RfxDocument {
    INDEX externalId: String,
    projectId: String,
    firmId: String,
    name: String,
    path: String,
    gcsPath: String,
    contentType: String,
    sizeBytes: I64 DEFAULT 0,
    createdAt: String,
    deletedAt: String DEFAULT "",
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

N::Content {
    INDEX externalId: String,
    projectId: String,
    firmId: String,
}

N::Document {
    INDEX externalId: String,
    documentSetId: String,
    projectId: String,
    firmId: String,
    name: String,
    gcsPath: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

N::Insights {
    INDEX externalId: String,
    projectId: String,
    firmId: String,
    insightsGcsBasePath: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
}

N::Insight {
    INDEX externalId: String,
    insightsId: String,
    name: String,
    gcsPath: String,
    projectId: String,
    firmId: String,
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

// -----------------------------------------------------------------------------
// USER CREDENTIALS & PROFILE
// -----------------------------------------------------------------------------

N::Education {
    INDEX externalId: String,
    userId: String,
    firmId: String,
    degree: String,
    major: String,
    university: String,
    graduationYear: I64 DEFAULT 0,
    createdAt: String,
    deletedAt: String DEFAULT "",
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

N::Certification {
    INDEX externalId: String,
    userId: String,
    firmId: String,
    name: String,
    description: String,
    issuingOrg: String,
    dateObtained: String,
    expirationDate: String,
    issuedAt: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

N::Registration {
    INDEX externalId: String,
    userId: String,
    firmId: String,
    name: String,
    issuedAt: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

N::ProjectExperience {
    INDEX externalId: String,
    userId: String,
    firmId: String,
    name: String,
    description: String,
    role: String,
    clientName: String,
    startDate: String,
    endDate: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

N::AreaOfExpertise {
    INDEX externalId: String,
    name: String,
}

N::ProjectManager {
    INDEX externalId: String,
    userId: String,
    firstName: String,
    lastName: String,
    email: String,
    jobTitle: String,
    designation: String,
    overview: String,
    areasOfExpertise: String,
}

N::AISuitability {
    INDEX externalId: String,
    projectId: String,
    firmId: String,
    decision: String,
    confidenceScore: F64 DEFAULT 0.0,
    reason: String,
    citationsPath: String,
    generatedAt: String,
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

N::TeamRecommendation {
    INDEX externalId: String,
    projectId: String,
    firmId: String,
    decision: String,
    confidenceScore: F64 DEFAULT 0.0,
    reason: String,
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

N::ClientContact {
    INDEX externalId: String,
    clientId: String,
    firmId: String,
    firstName: String,
    lastName: String,
    jobTitle: String,
    email: String,
    phone: String,
    linkedinUrl: String,
    notes: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
    vectorDocumentId: String DEFAULT "",
    vectorStoreStatus: String DEFAULT "",
}

N::ClientLocation {
    INDEX externalId: String,
    clientId: String,
    firmId: String,
    name: String,
    address: String,
    city: String,
    state: String,
    zip: String,
    country: String,
    email: String,
    phone: String,
    isBillingAddress: Boolean DEFAULT false,
    isPrimaryAddress: Boolean DEFAULT false,
    additionalInformation: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
}

N::ClientKeyRole {
    INDEX externalId: String,
    clientId: String,
    firmId: String,
    role: String,
    clientStaffMemberId: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
}

// -----------------------------------------------------------------------------
// PURSUIT / OPPORTUNITY
// -----------------------------------------------------------------------------

N::PursuitTeamMember {
    INDEX externalId: String,
    firmId: String,
    projectId: String,
    userId: String,
    role: String,
    roleType: String,
    startDate: String,
    endDate: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
}

N::PursuitTask {
    INDEX externalId: String,
    firmId: String,
    projectId: String,
    teamMemberId: String,
    description: String,
    status: String,
    creatorId: String,
    assigneeId: String,
    comments: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
}

N::PursuitLocation {
    INDEX externalId: String,
    projectId: String,
    firmId: String,
    name: String,
    address: String,
    city: String,
    state: String,
    zip: String,
    country: String,
    email: String,
    phone: String,
    additionalInformation: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
}

// -----------------------------------------------------------------------------
// PROJECT LOCATION
// -----------------------------------------------------------------------------

N::Location {
    INDEX externalId: String,
    projectId: String,
    firmId: String,
    name: String,
    address: String,
    city: String,
    state: String,
    zip: String,
    country: String,
    email: String,
    phone: String,
    createdAt: String,
    deletedAt: String DEFAULT "",
}

// -----------------------------------------------------------------------------
// PROPOSAL TEMPLATES
// -----------------------------------------------------------------------------

N::ProposalTemplate {
    INDEX externalId: String,
    firmId: String,
    name: String,
    clientName: String,
    gcsPath: String,
    sectionCount: I64 DEFAULT 0,
    orgTypeId: String,
    endMarkets: String,
    deletedAt: String DEFAULT "",
}

// =============================================================================
// VECTOR TYPE — replaces Turbopuffer namespace
// =============================================================================

V::DocumentChunk {
    externalId: String,
    documentId: String,
    firmId: String,
    projectId: String,
    sourceType: String,
    sourceId: String,
    chunkIndex: I64 DEFAULT 0,
    contentPreview: String,
    gcsContentPath: String,
    mimeType: String,
    vectorDocumentId: String,
    vectorStoreStatus: String,
    summary: String,
    sectionTitle: String,
    title: String,
    headingPath: String,
    pageNumber: I64 DEFAULT 0,
    proposalId: String,
    source: String,
    tokenCount: I64 DEFAULT 0,
    createdAt: String,
}

// N:: companion node for BM25 keyword search on document chunks.
// V:: types do not support SearchBM25, so text is stored in an N:: node
// and linked to V::DocumentChunk via E::ChunkTextHasVector.
N::DocumentChunkText {
    INDEX externalId: String,
    documentId: String,
    firmId: String,
    projectId: String,
    sourceType: String,
    sourceId: String,
    chunkIndex: I64 DEFAULT 0,
    contentPreview: String,
    summary: String,
    sectionTitle: String,
    title: String,
    headingPath: String,
}

// =============================================================================
// EDGES — decomposed from Memgraph's polymorphic HAS/CONTAINS + typed edges
//
// Naming convention: <Verb> or <SourceTarget> for decomposed polymorphic edges
// =============================================================================

// --- Firm ownership (from HAS) ---

E::FirmHasUser {
    From: Firm,
    To: User,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::FirmHasProject {
    From: Firm,
    To: Project,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::FirmHasClient {
    From: Firm,
    To: Client,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::FirmHasVendor {
    From: Firm,
    To: Vendor,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::FirmHasFilesRoot UNIQUE {
    From: Firm,
    To: FilesRoot,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::FirmHasProposalTemplate {
    From: Firm,
    To: ProposalTemplate,
    Properties: {
        v: String DEFAULT "1",
    }
}

// --- Proposal structure (from HAS) ---

E::ProjectHasProposal UNIQUE {
    From: Project,
    To: Proposal,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::ProposalHasEvaluation UNIQUE {
    From: Proposal,
    To: Evaluation,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::ProposalHasRfxDocuments UNIQUE {
    From: Proposal,
    To: RfxDocuments,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::ProposalHasContent UNIQUE {
    From: Proposal,
    To: Content,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::ProposalHasInsights UNIQUE {
    From: Proposal,
    To: Insights,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::EvaluationHasSection {
    From: Evaluation,
    To: EvaluationSection,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::InsightsHasInsight {
    From: Insights,
    To: Insight,
    Properties: {
        v: String DEFAULT "1",
    }
}

// --- User profile (from HAS) ---

E::UserHasExpertise {
    From: User,
    To: AreaOfExpertise,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::UserHasRegistration {
    From: User,
    To: Registration,
    Properties: {
        v: String DEFAULT "1",
    }
}

// --- Client structure (from HAS) ---

E::ClientHasContact {
    From: Client,
    To: ClientContact,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::ClientHasKeyRole {
    From: Client,
    To: ClientKeyRole,
    Properties: {
        v: String DEFAULT "1",
    }
}

// --- File system (from CONTAINS) ---

E::FilesRootContainsFolder {
    From: FilesRoot,
    To: Folder,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::FilesRootContainsFile {
    From: FilesRoot,
    To: File,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::FolderContainsFolder {
    From: Folder,
    To: Folder,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::FolderContainsFile {
    From: Folder,
    To: File,
    Properties: {
        v: String DEFAULT "1",
    }
}

// --- Proposal documents (from CONTAINS) ---

E::ContentContainsDocument {
    From: Content,
    To: Document,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::RfxDocsContainsRfxDoc {
    From: RfxDocuments,
    To: RfxDocument,
    Properties: {
        v: String DEFAULT "1",
    }
}

// --- Already-typed edges (1:1 from Memgraph) ---

E::HasClient {
    From: Project,
    To: Client,
    Properties: {
        role: String,
        v: String DEFAULT "1",
    }
}

E::EngagedOn {
    From: Client,
    To: Project,
    Properties: {
        role: String,
        targetExternalId: String DEFAULT "",
        v: String DEFAULT "1",
    }
}

E::LedBy UNIQUE {
    From: Project,
    To: ProjectManager,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::HasSuitability UNIQUE {
    From: Project,
    To: AISuitability,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::HasRecommendation UNIQUE {
    From: Project,
    To: TeamRecommendation,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::ConductedAt {
    From: Project,
    To: Location,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::HasFile {
    From: Project,
    To: File,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::Attended {
    From: User,
    To: Education,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::Completed {
    From: User,
    To: Certification,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::WorkedOn {
    From: User,
    To: ProjectExperience,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::IsA UNIQUE {
    From: User,
    To: ProjectManager,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::ChildOf UNIQUE {
    From: Client,
    To: Client,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::LocatedAt {
    From: Client,
    To: ClientLocation,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::FilledBy {
    From: ClientKeyRole,
    To: ClientContact,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::HasTeamMember {
    From: Project,
    To: PursuitTeamMember,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::IsUser {
    From: PursuitTeamMember,
    To: User,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::UserAssignedToTeamMember {
    From: User,
    To: PursuitTeamMember,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::ContactAssignedToProject {
    From: ClientContact,
    To: Project,
    Properties: {
        role: String,
        v: String DEFAULT "1",
        targetExternalId: String DEFAULT "",
    }
}

E::WorksOnProject {
    From: User,
    To: Project,
    Properties: {
        v: String DEFAULT "1",
        targetExternalId: String DEFAULT "",
    }
}

E::HasTask {
    From: PursuitTeamMember,
    To: PursuitTask,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::HasPursuitLocation {
    From: Project,
    To: PursuitLocation,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::ProjectHasClientContact {
    From: Project,
    To: ClientContact,
    Properties: {
        role: String,
        v: String DEFAULT "1",
        targetExternalId: String DEFAULT "",
    }
}

E::DerivedTemplate {
    From: Project,
    To: ProposalTemplate,
    Properties: {
        v: String DEFAULT "1",
    }
}

E::ChunkTextHasVector {
    From: DocumentChunkText,
    To: DocumentChunk,
    Properties: {
        v: String DEFAULT "1",
    }
}
