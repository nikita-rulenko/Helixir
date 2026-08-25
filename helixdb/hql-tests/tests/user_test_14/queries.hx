// =============================================================================
// SECTION 1: NODE CREATION
// =============================================================================

// --- Core entities ---

QUERY AddFirm(
    externalId: String, name: String, description: String,
    domain: String, email: String, phone: String,
    city: String, state: String,
    createdAt: String, deletedAt: String
) =>
    result <- AddN<Firm>({
        externalId: externalId, name: name, description: description,
        domain: domain, email: email, phone: phone,
        city: city, state: state,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY AddUser(
    externalId: String, firmId: String, firstName: String, lastName: String,
    email: String, phone: String, designation: String, jobTitle: String,
    category: String, accountType: String, admin: Boolean,
    areasOfExpertise: String, overview: String, yearsOfExperience: I64,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<User>({
        externalId: externalId, firmId: firmId, firstName: firstName, lastName: lastName,
        email: email, phone: phone, designation: designation, jobTitle: jobTitle,
        category: category, accountType: accountType, admin: admin,
        areasOfExpertise: areasOfExpertise, overview: overview,
        yearsOfExperience: yearsOfExperience,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY AddProject(
    externalId: String, firmId: String, clientId: String, billingClientId: String,
    name: String, description: String, referenceNumber: String,
    location: String, clientName: String,
    projectManagerName: String, projectManagerId: String,
    budget: F64, startDate: String, endDate: String, status: String,
    entityClassification: String, createdAt: String, deletedAt: String,
    suitabilityDecision: String, suitabilityConfidenceScore: F64,
    suitabilityReason: String, suitabilityGeneratedAt: String,
    recommendationDecision: String, recommendationConfidenceScore: F64,
    pursuitStatus: String, pursuitPriority: String, pursuitArchived: Boolean,
    serviceLines: String, endMarkets: String,
    opportunitySourceCategory: String, opportunitySourceDetails: String,
    responsibility: String, opportunityUrl: String, opportunityDescription: String,
    estimatedFee: F64, estimatedConstructionCost: F64, expectedRevenue: F64,
    announcementDate: String, rfpIssueDate: String, submissionsOpenDate: String,
    questionsDueDate: String, bidDate: String, interviewDate: String,
    awardAnnouncementDate: String, noticeToProceedDate: String,
    expectedStartDate: String, expectedEndDate: String
) =>
    result <- AddN<Project>({
        externalId: externalId, firmId: firmId, clientId: clientId, billingClientId: billingClientId,
        name: name, description: description, referenceNumber: referenceNumber,
        location: location, clientName: clientName,
        projectManagerName: projectManagerName, projectManagerId: projectManagerId,
        budget: budget, startDate: startDate, endDate: endDate, status: status,
        entityClassification: entityClassification, createdAt: createdAt, deletedAt: deletedAt,
        suitabilityDecision: suitabilityDecision,
        suitabilityConfidenceScore: suitabilityConfidenceScore,
        suitabilityReason: suitabilityReason,
        suitabilityGeneratedAt: suitabilityGeneratedAt,
        recommendationDecision: recommendationDecision,
        recommendationConfidenceScore: recommendationConfidenceScore,
        pursuitStatus: pursuitStatus, pursuitPriority: pursuitPriority,
        pursuitArchived: pursuitArchived,
        serviceLines: serviceLines, endMarkets: endMarkets,
        opportunitySourceCategory: opportunitySourceCategory,
        opportunitySourceDetails: opportunitySourceDetails,
        responsibility: responsibility, opportunityUrl: opportunityUrl,
        opportunityDescription: opportunityDescription,
        estimatedFee: estimatedFee, estimatedConstructionCost: estimatedConstructionCost,
        expectedRevenue: expectedRevenue,
        announcementDate: announcementDate, rfpIssueDate: rfpIssueDate,
        submissionsOpenDate: submissionsOpenDate, questionsDueDate: questionsDueDate,
        bidDate: bidDate, interviewDate: interviewDate,
        awardAnnouncementDate: awardAnnouncementDate,
        noticeToProceedDate: noticeToProceedDate,
        expectedStartDate: expectedStartDate, expectedEndDate: expectedEndDate
    })
    RETURN result

QUERY AddVendor(
    externalId: String, firmId: String, name: String, description: String,
    createdAt: String, deletedAt: String
) =>
    result <- AddN<Vendor>({
        externalId: externalId, firmId: firmId, name: name, description: description,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY AddClient(
    externalId: String, firmId: String, name: String, referenceNumber: String,
    description: String, website: String, email: String, phone: String,
    status: String, clientType: String, clientSubType: String,
    market: String, governmentAgency: String, parentClientId: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<Client>({
        externalId: externalId, firmId: firmId, name: name, referenceNumber: referenceNumber,
        description: description, website: website, email: email, phone: phone,
        status: status, clientType: clientType, clientSubType: clientSubType,
        market: market, governmentAgency: governmentAgency,
        parentClientId: parentClientId,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

// --- File system ---

QUERY AddFilesRoot(externalId: String, firmId: String, name: String) =>
    result <- AddN<FilesRoot>({externalId: externalId, firmId: firmId, name: name})
    RETURN result

QUERY AddFolder(
    externalId: String, firmId: String, parentId: String,
    name: String, path: String,
    createdAt: String, deletedAt: String
) =>
    result <- AddN<Folder>({
        externalId: externalId, firmId: firmId, parentId: parentId,
        name: name, path: path,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY AddFile(
    externalId: String, firmId: String, folderId: String, folderPath: String,
    name: String, path: String, gcsPath: String, mimeType: String,
    sizeBytes: I64, projectId: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<File>({
        externalId: externalId, firmId: firmId, folderId: folderId, folderPath: folderPath,
        name: name, path: path, gcsPath: gcsPath, mimeType: mimeType,
        sizeBytes: sizeBytes, projectId: projectId,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

// --- Proposal structure ---

QUERY AddProposal(
    externalId: String, projectId: String, firmId: String,
    createdAt: String, deletedAt: String
) =>
    result <- AddN<Proposal>({
        externalId: externalId, projectId: projectId, firmId: firmId,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY AddEvaluation(
    externalId: String, projectId: String, firmId: String,
    sectionsGcsBasePath: String, createdAt: String, deletedAt: String
) =>
    result <- AddN<Evaluation>({
        externalId: externalId, projectId: projectId, firmId: firmId,
        sectionsGcsBasePath: sectionsGcsBasePath,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY AddEvaluationSection(
    externalId: String, evaluationId: String, name: String,
    gcsPath: String, projectId: String, firmId: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<EvaluationSection>({
        externalId: externalId, evaluationId: evaluationId, name: name,
        gcsPath: gcsPath, projectId: projectId, firmId: firmId,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY AddRfxDocumentsContainer(externalId: String, projectId: String, firmId: String) =>
    result <- AddN<RfxDocuments>({externalId: externalId, projectId: projectId, firmId: firmId})
    RETURN result

QUERY AddRfxDocument(
    externalId: String, projectId: String, firmId: String,
    name: String, path: String, gcsPath: String,
    contentType: String, sizeBytes: I64,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<RfxDocument>({
        externalId: externalId, projectId: projectId, firmId: firmId,
        name: name, path: path, gcsPath: gcsPath,
        contentType: contentType, sizeBytes: sizeBytes,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY AddContentContainer(externalId: String, projectId: String, firmId: String) =>
    result <- AddN<Content>({externalId: externalId, projectId: projectId, firmId: firmId})
    RETURN result

QUERY AddDocument(
    externalId: String, documentSetId: String, projectId: String, firmId: String,
    name: String, gcsPath: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<Document>({
        externalId: externalId, documentSetId: documentSetId, projectId: projectId, firmId: firmId,
        name: name, gcsPath: gcsPath,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY AddInsightsContainer(
    externalId: String, projectId: String, firmId: String,
    insightsGcsBasePath: String, createdAt: String, deletedAt: String
) =>
    result <- AddN<Insights>({
        externalId: externalId, projectId: projectId, firmId: firmId,
        insightsGcsBasePath: insightsGcsBasePath,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY AddInsight(
    externalId: String, insightsId: String, name: String,
    gcsPath: String, projectId: String, firmId: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<Insight>({
        externalId: externalId, insightsId: insightsId, name: name,
        gcsPath: gcsPath, projectId: projectId, firmId: firmId,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

// --- User credentials & profile ---

QUERY AddEducation(
    externalId: String, userId: String, firmId: String,
    degree: String, major: String, university: String,
    graduationYear: I64, createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<Education>({
        externalId: externalId, userId: userId, firmId: firmId,
        degree: degree, major: major, university: university,
        graduationYear: graduationYear, createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY AddCertification(
    externalId: String, userId: String, firmId: String,
    name: String, description: String, issuingOrg: String,
    dateObtained: String, expirationDate: String, issuedAt: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<Certification>({
        externalId: externalId, userId: userId, firmId: firmId,
        name: name, description: description, issuingOrg: issuingOrg,
        dateObtained: dateObtained, expirationDate: expirationDate,
        issuedAt: issuedAt, createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY AddRegistration(
    externalId: String, userId: String, firmId: String,
    name: String, issuedAt: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<Registration>({
        externalId: externalId, userId: userId, firmId: firmId,
        name: name, issuedAt: issuedAt,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY AddProjectExperience(
    externalId: String, userId: String, firmId: String,
    name: String, description: String, role: String, clientName: String,
    startDate: String, endDate: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<ProjectExperience>({
        externalId: externalId, userId: userId, firmId: firmId,
        name: name, description: description, role: role, clientName: clientName,
        startDate: startDate, endDate: endDate,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY AddAreaOfExpertise(externalId: String, name: String) =>
    result <- AddN<AreaOfExpertise>({externalId: externalId, name: name})
    RETURN result

QUERY AddProjectManager(
    externalId: String, userId: String,
    firstName: String, lastName: String, email: String,
    jobTitle: String, designation: String,
    overview: String, areasOfExpertise: String
) =>
    result <- AddN<ProjectManager>({
        externalId: externalId, userId: userId,
        firstName: firstName, lastName: lastName, email: email,
        jobTitle: jobTitle, designation: designation,
        overview: overview, areasOfExpertise: areasOfExpertise
    })
    RETURN result

// --- AI analysis ---

QUERY AddAISuitability(
    externalId: String, projectId: String, firmId: String,
    decision: String, confidenceScore: F64, reason: String,
    citationsPath: String, generatedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<AISuitability>({
        externalId: externalId, projectId: projectId, firmId: firmId,
        decision: decision, confidenceScore: confidenceScore, reason: reason,
        citationsPath: citationsPath, generatedAt: generatedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY AddTeamRecommendation(
    externalId: String, projectId: String, firmId: String,
    decision: String, confidenceScore: F64, reason: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<TeamRecommendation>({
        externalId: externalId, projectId: projectId, firmId: firmId,
        decision: decision, confidenceScore: confidenceScore, reason: reason,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

// --- Client details ---

QUERY AddClientContact(
    externalId: String, clientId: String, firmId: String,
    firstName: String, lastName: String, jobTitle: String,
    email: String, phone: String, linkedinUrl: String, notes: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    result <- AddN<ClientContact>({
        externalId: externalId, clientId: clientId, firmId: firmId,
        firstName: firstName, lastName: lastName, jobTitle: jobTitle,
        email: email, phone: phone, linkedinUrl: linkedinUrl, notes: notes,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY AddClientLocation(
    externalId: String, clientId: String, firmId: String,
    name: String, address: String, city: String, state: String,
    zip: String, country: String, email: String, phone: String,
    isBillingAddress: Boolean, isPrimaryAddress: Boolean,
    additionalInformation: String,
    createdAt: String, deletedAt: String
) =>
    result <- AddN<ClientLocation>({
        externalId: externalId, clientId: clientId, firmId: firmId,
        name: name, address: address, city: city, state: state,
        zip: zip, country: country, email: email, phone: phone,
        isBillingAddress: isBillingAddress, isPrimaryAddress: isPrimaryAddress,
        additionalInformation: additionalInformation,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY AddClientKeyRole(
    externalId: String, clientId: String, firmId: String,
    role: String, clientStaffMemberId: String,
    createdAt: String, deletedAt: String
) =>
    result <- AddN<ClientKeyRole>({
        externalId: externalId, clientId: clientId, firmId: firmId,
        role: role, clientStaffMemberId: clientStaffMemberId,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

// --- Pursuit ---

QUERY AddPursuitTeamMember(
    externalId: String, firmId: String, projectId: String, userId: String,
    role: String, roleType: String, startDate: String, endDate: String,
    createdAt: String, deletedAt: String
) =>
    result <- AddN<PursuitTeamMember>({
        externalId: externalId, firmId: firmId, projectId: projectId, userId: userId,
        role: role, roleType: roleType, startDate: startDate, endDate: endDate,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY AddPursuitTask(
    externalId: String, firmId: String, projectId: String, teamMemberId: String,
    description: String, status: String,
    creatorId: String, assigneeId: String, comments: String,
    createdAt: String, deletedAt: String
) =>
    result <- AddN<PursuitTask>({
        externalId: externalId, firmId: firmId, projectId: projectId, teamMemberId: teamMemberId,
        description: description, status: status,
        creatorId: creatorId, assigneeId: assigneeId, comments: comments,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY AddPursuitLocation(
    externalId: String, projectId: String, firmId: String,
    name: String, address: String, city: String, state: String,
    zip: String, country: String, email: String, phone: String,
    additionalInformation: String,
    createdAt: String, deletedAt: String
) =>
    result <- AddN<PursuitLocation>({
        externalId: externalId, projectId: projectId, firmId: firmId,
        name: name, address: address, city: city, state: state,
        zip: zip, country: country, email: email, phone: phone,
        additionalInformation: additionalInformation,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

// --- Project location ---

QUERY AddLocation(
    externalId: String, projectId: String, firmId: String,
    name: String, address: String, city: String, state: String,
    zip: String, country: String, email: String, phone: String,
    createdAt: String, deletedAt: String
) =>
    result <- AddN<Location>({
        externalId: externalId, projectId: projectId, firmId: firmId,
        name: name, address: address, city: city, state: state,
        zip: zip, country: country, email: email, phone: phone,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

// --- Proposal templates ---

QUERY AddProposalTemplate(
    externalId: String, firmId: String, name: String,
    clientName: String, gcsPath: String, sectionCount: I64,
    orgTypeId: String, endMarkets: String, deletedAt: String
) =>
    result <- AddN<ProposalTemplate>({
        externalId: externalId, firmId: firmId, name: name,
        clientName: clientName, gcsPath: gcsPath, sectionCount: sectionCount,
        orgTypeId: orgTypeId, endMarkets: endMarkets, deletedAt: deletedAt
    })
    RETURN result

// --- Document chunk text (BM25 companion node for V::DocumentChunk) ---

QUERY AddDocumentChunkText(
    externalId: String, documentId: String, firmId: String, projectId: String,
    sourceType: String, sourceId: String, chunkIndex: I64,
    contentPreview: String,
    summary: String, sectionTitle: String, title: String, headingPath: String
) =>
    result <- AddN<DocumentChunkText>({
        externalId: externalId, documentId: documentId, firmId: firmId,
        projectId: projectId, sourceType: sourceType, sourceId: sourceId,
        chunkIndex: chunkIndex, contentPreview: contentPreview,
        summary: summary, sectionTitle: sectionTitle, title: title,
        headingPath: headingPath
    })
    RETURN result

// =============================================================================
// SECTION 2: EDGE CREATION (Seed)
//
// Edge queries resolve nodes by externalId (indexed secondary lookup) then
// create edges using the resolved HelixDB internal IDs. This eliminates the
// need for the seed script to capture and map internal IDs.
// =============================================================================

// --- Firm ownership ---
QUERY LinkFirmUser(firmId: String, userId: String) =>
    from <- N<Firm>({externalId: firmId})
    to <- N<User>({externalId: userId})
    result <- AddE<FirmHasUser>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkFirmProject(firmId: String, projectId: String) =>
    from <- N<Firm>({externalId: firmId})
    to <- N<Project>({externalId: projectId})
    result <- AddE<FirmHasProject>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkFirmClient(firmId: String, clientId: String) =>
    from <- N<Firm>({externalId: firmId})
    to <- N<Client>({externalId: clientId})
    result <- AddE<FirmHasClient>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkFirmVendor(firmId: String, vendorId: String) =>
    from <- N<Firm>({externalId: firmId})
    to <- N<Vendor>({externalId: vendorId})
    result <- AddE<FirmHasVendor>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkFirmFilesRoot(firmId: String, filesRootId: String) =>
    from <- N<Firm>({externalId: firmId})
    to <- N<FilesRoot>({externalId: filesRootId})
    result <- AddE<FirmHasFilesRoot>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkFirmProposalTemplate(firmId: String, templateId: String) =>
    from <- N<Firm>({externalId: firmId})
    to <- N<ProposalTemplate>({externalId: templateId})
    result <- AddE<FirmHasProposalTemplate>({v: "1"})::From(from)::To(to)
    RETURN result

// --- Proposal structure ---
QUERY LinkProjectProposal(projectId: String, proposalId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<Proposal>({externalId: proposalId})
    result <- AddE<ProjectHasProposal>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkProposalEvaluation(proposalId: String, evaluationId: String) =>
    from <- N<Proposal>({externalId: proposalId})
    to <- N<Evaluation>({externalId: evaluationId})
    result <- AddE<ProposalHasEvaluation>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkProposalRfxDocuments(proposalId: String, rfxDocsId: String) =>
    from <- N<Proposal>({externalId: proposalId})
    to <- N<RfxDocuments>({externalId: rfxDocsId})
    result <- AddE<ProposalHasRfxDocuments>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkProposalContent(proposalId: String, contentId: String) =>
    from <- N<Proposal>({externalId: proposalId})
    to <- N<Content>({externalId: contentId})
    result <- AddE<ProposalHasContent>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkProposalInsights(proposalId: String, insightsId: String) =>
    from <- N<Proposal>({externalId: proposalId})
    to <- N<Insights>({externalId: insightsId})
    result <- AddE<ProposalHasInsights>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkEvaluationSection(evaluationId: String, sectionId: String) =>
    from <- N<Evaluation>({externalId: evaluationId})
    to <- N<EvaluationSection>({externalId: sectionId})
    result <- AddE<EvaluationHasSection>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkInsightsInsight(insightsId: String, insightId: String) =>
    from <- N<Insights>({externalId: insightsId})
    to <- N<Insight>({externalId: insightId})
    result <- AddE<InsightsHasInsight>({v: "1"})::From(from)::To(to)
    RETURN result

// --- User profile ---
QUERY LinkUserExpertise(userId: String, expertiseId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<AreaOfExpertise>({externalId: expertiseId})
    result <- AddE<UserHasExpertise>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkUserRegistration(userId: String, registrationId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<Registration>({externalId: registrationId})
    result <- AddE<UserHasRegistration>({v: "1"})::From(from)::To(to)
    RETURN result

// --- Client structure ---
QUERY LinkClientContact(clientId: String, contactId: String) =>
    from <- N<Client>({externalId: clientId})
    to <- N<ClientContact>({externalId: contactId})
    result <- AddE<ClientHasContact>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkClientKeyRole(clientId: String, keyRoleId: String) =>
    from <- N<Client>({externalId: clientId})
    to <- N<ClientKeyRole>({externalId: keyRoleId})
    result <- AddE<ClientHasKeyRole>({v: "1"})::From(from)::To(to)
    RETURN result

// --- File system ---
QUERY LinkFilesRootFolder(filesRootId: String, folderId: String) =>
    from <- N<FilesRoot>({externalId: filesRootId})
    to <- N<Folder>({externalId: folderId})
    result <- AddE<FilesRootContainsFolder>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkFilesRootFile(filesRootId: String, fileId: String) =>
    from <- N<FilesRoot>({externalId: filesRootId})
    to <- N<File>({externalId: fileId})
    result <- AddE<FilesRootContainsFile>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkFolderFolder(parentFolderId: String, childFolderId: String) =>
    from <- N<Folder>({externalId: parentFolderId})
    to <- N<Folder>({externalId: childFolderId})
    result <- AddE<FolderContainsFolder>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkFolderFile(folderId: String, fileId: String) =>
    from <- N<Folder>({externalId: folderId})
    to <- N<File>({externalId: fileId})
    result <- AddE<FolderContainsFile>({v: "1"})::From(from)::To(to)
    RETURN result

// --- Proposal documents ---
QUERY LinkContentDocument(contentId: String, documentId: String) =>
    from <- N<Content>({externalId: contentId})
    to <- N<Document>({externalId: documentId})
    result <- AddE<ContentContainsDocument>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkRfxDocsRfxDoc(rfxDocsId: String, rfxDocId: String) =>
    from <- N<RfxDocuments>({externalId: rfxDocsId})
    to <- N<RfxDocument>({externalId: rfxDocId})
    result <- AddE<RfxDocsContainsRfxDoc>({v: "1"})::From(from)::To(to)
    RETURN result

// --- Typed edges (with properties) ---
QUERY LinkProjectClient(projectId: String, clientId: String, role: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<Client>({externalId: clientId})
    result <- AddE<HasClient>({role: role, v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkClientProject(clientId: String, projectId: String, role: String) =>
    from <- N<Client>({externalId: clientId})
    to <- N<Project>({externalId: projectId})
    result <- AddE<EngagedOn>({role: role, v: "1", targetExternalId: projectId})::From(from)::To(to)
    RETURN result

QUERY LinkProjectManager(projectId: String, pmId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<ProjectManager>({externalId: pmId})
    result <- AddE<LedBy>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkProjectSuitability(projectId: String, suitabilityId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<AISuitability>({externalId: suitabilityId})
    result <- AddE<HasSuitability>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkProjectRecommendation(projectId: String, recommendationId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<TeamRecommendation>({externalId: recommendationId})
    result <- AddE<HasRecommendation>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkProjectLocation(projectId: String, locationId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<Location>({externalId: locationId})
    result <- AddE<ConductedAt>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkProjectFile(projectId: String, fileId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<File>({externalId: fileId})
    result <- AddE<HasFile>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkUserEducation(userId: String, educationId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<Education>({externalId: educationId})
    result <- AddE<Attended>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkUserCertification(userId: String, certificationId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<Certification>({externalId: certificationId})
    result <- AddE<Completed>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkUserProjectExperience(userId: String, experienceId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<ProjectExperience>({externalId: experienceId})
    result <- AddE<WorkedOn>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkUserIsProjectManager(userId: String, pmId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<ProjectManager>({externalId: pmId})
    result <- AddE<IsA>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkClientChild(childClientId: String, parentClientId: String) =>
    from <- N<Client>({externalId: childClientId})
    to <- N<Client>({externalId: parentClientId})
    result <- AddE<ChildOf>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkClientLocation(clientId: String, locationId: String) =>
    from <- N<Client>({externalId: clientId})
    to <- N<ClientLocation>({externalId: locationId})
    result <- AddE<LocatedAt>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkKeyRoleContact(keyRoleId: String, contactId: String) =>
    from <- N<ClientKeyRole>({externalId: keyRoleId})
    to <- N<ClientContact>({externalId: contactId})
    result <- AddE<FilledBy>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkProjectTeamMember(projectId: String, teamMemberId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<PursuitTeamMember>({externalId: teamMemberId})
    result <- AddE<HasTeamMember>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkTeamMemberUser(teamMemberId: String, userId: String) =>
    from <- N<PursuitTeamMember>({externalId: teamMemberId})
    to <- N<User>({externalId: userId})
    result <- AddE<IsUser>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkUserToTeamMember(userId: String, teamMemberId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<PursuitTeamMember>({externalId: teamMemberId})
    result <- AddE<UserAssignedToTeamMember>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkContactToProject(contactId: String, projectId: String, role: String) =>
    from <- N<ClientContact>({externalId: contactId})
    to <- N<Project>({externalId: projectId})
    result <- AddE<ContactAssignedToProject>({role: role, v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkTeamMemberTask(teamMemberId: String, taskId: String) =>
    from <- N<PursuitTeamMember>({externalId: teamMemberId})
    to <- N<PursuitTask>({externalId: taskId})
    result <- AddE<HasTask>({v: "1"})::From(from)::To(to)
    RETURN result

QUERY LinkProjectPursuitLocation(projectId: String, pursuitLocationId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<PursuitLocation>({externalId: pursuitLocationId})
    result <- AddE<HasPursuitLocation>({v: "1"})::From(from)::To(to)
    RETURN result

// =============================================================================
// SECTION 3: VECTOR & BM25 OPERATIONS (Seed + Search)
// =============================================================================

QUERY AddDocumentChunk(
    documentId: String, firmId: String, projectId: String,
    sourceType: String, sourceId: String, chunkIndex: I64,
    contentPreview: String, gcsContentPath: String,
    mimeType: String, vectorDocumentId: String, vectorStoreStatus: String,
    summary: String, sectionTitle: String, title: String, headingPath: String,
    pageNumber: I64, proposalId: String, source: String,
    tokenCount: I64, createdAt: String,
    embedding: [F64]
) =>
    result <- AddV<DocumentChunk>(embedding, {
        documentId: documentId, firmId: firmId, projectId: projectId,
        sourceType: sourceType, sourceId: sourceId, chunkIndex: chunkIndex,
        contentPreview: contentPreview, gcsContentPath: gcsContentPath,
        mimeType: mimeType, vectorDocumentId: vectorDocumentId,
        vectorStoreStatus: vectorStoreStatus, summary: summary,
        sectionTitle: sectionTitle, title: title, headingPath: headingPath,
        pageNumber: pageNumber, proposalId: proposalId, source: source,
        tokenCount: tokenCount, createdAt: createdAt
    })
    RETURN result

// Combined ingestion: creates N::DocumentChunkText (BM25) + V::DocumentChunk (ANN)
// + E::ChunkTextHasVector edge in a single query. Use this for new chunk ingestion.
QUERY IngestDocumentChunk(
    externalId: String, documentId: String, firmId: String, projectId: String,
    sourceType: String, sourceId: String, chunkIndex: I64,
    contentPreview: String, gcsContentPath: String,
    mimeType: String, vectorDocumentId: String, vectorStoreStatus: String,
    summary: String, sectionTitle: String, title: String, headingPath: String,
    pageNumber: I64, proposalId: String, source: String,
    tokenCount: I64, createdAt: String,
    embedding: [F64]
) =>
    textNode <- AddN<DocumentChunkText>({
        externalId: externalId, documentId: documentId, firmId: firmId,
        projectId: projectId, sourceType: sourceType, sourceId: sourceId,
        chunkIndex: chunkIndex, contentPreview: contentPreview,
        summary: summary, sectionTitle: sectionTitle, title: title,
        headingPath: headingPath
    })
    vector <- AddV<DocumentChunk>(embedding, {
        documentId: documentId, firmId: firmId, projectId: projectId,
        sourceType: sourceType, sourceId: sourceId, chunkIndex: chunkIndex,
        contentPreview: contentPreview, gcsContentPath: gcsContentPath,
        mimeType: mimeType, vectorDocumentId: vectorDocumentId,
        vectorStoreStatus: vectorStoreStatus, summary: summary,
        sectionTitle: sectionTitle, title: title, headingPath: headingPath,
        pageNumber: pageNumber, proposalId: proposalId, source: source,
        tokenCount: tokenCount, createdAt: createdAt
    })
    edge <- AddE<ChunkTextHasVector>({v: "1"})::From(textNode)::To(vector)
    RETURN textNode

// Vector similarity search (replaces Turbopuffer ANN)
#[mcp]
QUERY SearchChunksVector(queryVector: [F64], limit: I64) =>
    results <- SearchV<DocumentChunk>(queryVector, limit)::RerankRRF(k: 60)
    RETURN results

// BM25 keyword search on N::DocumentChunkText (companion node for V::DocumentChunk).
// The app layer calls SearchChunksVector + SearchChunksBM25 in parallel,
// then applies RRF fusion (k=60) and Cohere reranking — matching the
// existing TurboPuffer hybrid search pipeline.

#[mcp]
QUERY SearchChunksBM25(query: String, limit: I64) =>
    results <- SearchBM25<DocumentChunkText>(query, limit)
    RETURN results

// --- Firm-scoped search queries (server-side firmId filtering) ---
// HelixDB supports ::WHERE postfiltering on SearchV and SearchBM25 results.
// This eliminates the need for app-layer firmId filtering.

#[mcp]
QUERY SearchChunksVectorByFirm(queryVector: [F64], firmId: String, limit: I64) =>
    results <- SearchV<DocumentChunk>(queryVector, limit)::RerankRRF(k: 60)::WHERE(_::{firmId}::EQ(firmId))
    RETURN results

#[mcp]
QUERY SearchChunksBM25ByFirm(query: String, firmId: String, limit: I64) =>
    searched <- SearchBM25<DocumentChunkText>(query, limit)
    results <- searched::WHERE(_::{firmId}::EQ(firmId))
    RETURN results

// --- Document-scoped vector search (for context retrieval within a single document) ---

#[mcp]
QUERY SearchChunksVectorByDocument(queryVector: [F64], documentId: String, limit: I64) =>
    results <- SearchV<DocumentChunk>(queryVector, limit)::RerankRRF(k: 60)::WHERE(_::{documentId}::EQ(documentId))
    RETURN results

// --- Project-scoped vector search (for context retrieval within a project) ---

#[mcp]
QUERY SearchChunksVectorByProject(queryVector: [F64], projectId: String, limit: I64) =>
    results <- SearchV<DocumentChunk>(queryVector, limit)::RerankRRF(k: 60)::WHERE(_::{projectId}::EQ(projectId))
    RETURN results

// --- Source-scoped search (for finding chunks from a specific source entity) ---

#[mcp]
QUERY SearchChunksVectorBySource(queryVector: [F64], sourceType: String, sourceId: String, limit: I64) =>
    results <- SearchV<DocumentChunk>(queryVector, limit)::RerankRRF(k: 60)::WHERE(_::{sourceType}::EQ(sourceType))::WHERE(_::{sourceId}::EQ(sourceId))
    RETURN results

#[mcp]
QUERY SearchChunksBM25ByProject(query: String, projectId: String, limit: I64) =>
    searched <- SearchBM25<DocumentChunkText>(query, limit)
    results <- searched::WHERE(_::{projectId}::EQ(projectId))
    RETURN results
// =============================================================================
// SECTION 4: NODE LOOKUPS
//
// All #[mcp] queries accept Postgres UUIDs and look up via N<Type>({externalId}).
// RETURN ::!{ id, label } strips HelixDB internals so `externalId` is
// the only identifier in responses.
// =============================================================================
// =============================================================================

// --- List all firms (AI agent discovery: resolve firm name → firmId) ---

#[mcp]
QUERY GetAllFirms(start: U32, end: U32) =>
    result <- N<Firm>::WHERE(_::{deletedAt}::EQ(""))::RANGE(start, end)
    RETURN result::!{ id, label }

#[mcp]
QUERY GetProject(projectId: String) =>
    result <- N<Project>({externalId: projectId})
    RETURN result::!{ id, label }

#[mcp]
QUERY GetFirm(firmId: String) =>
    result <- N<Firm>({externalId: firmId})
    RETURN result::!{ id, label }

#[mcp]
QUERY GetUser(userId: String) =>
    result <- N<User>({externalId: userId})
    RETURN result::!{ id, label }

#[mcp]
QUERY GetClient(clientId: String) =>
    result <- N<Client>({externalId: clientId})
    RETURN result::!{ id, label }

#[mcp]
QUERY GetFolder(folderId: String) =>
    result <- N<Folder>({externalId: folderId})
    RETURN result::!{ id, label }

#[mcp]
QUERY GetFileById(fileId: String) =>
    result <- N<File>({externalId: fileId})
    RETURN result::!{ id, label }

// =============================================================================
// SECTION 5: GRAPH TRAVERSALS (replaces OPTIONAL MATCH branches)
//
// Each query maps to one OPTIONAL MATCH branch in the original Cypher templates.
// Input: Postgres UUID (externalId) → looks up start node → traverses edges.
// Output: RETURN result::!{ id, label } — strips HelixDB internals.
// =============================================================================

// --- Project → Proposal (used by: proposal.*, chat.project, pursuit.*) ---

#[mcp]
QUERY GetProjectProposal(projectId: String) =>
    result <- N<Project>({externalId: projectId})::Out<ProjectHasProposal>
    RETURN result::!{ id, label }

// --- Proposal → Evaluation → EvaluationSection ---
// Used by: proposal.evaluation, proposal.content, proposal.chat, proposal.scoring,
//          proposal.*.fixed, pursuit.suitability.fixed

#[mcp]
QUERY GetProposalEvalSections(projectId: String) =>
    result <- N<Project>({externalId: projectId})
    ::Out<ProjectHasProposal>
    ::Out<ProposalHasEvaluation>
    ::Out<EvaluationHasSection>
    RETURN result::!{ id, label }

// --- Proposal → RfxDocuments → RfxDocument ---
// Used by: proposal.evaluation, proposal.content, proposal.outline,
//          proposal.chat, proposal.scoring, chat.project, all fixed variants

#[mcp]
QUERY GetProposalRfxDocs(projectId: String) =>
    result <- N<Project>({externalId: projectId})
    ::Out<ProjectHasProposal>
    ::Out<ProposalHasRfxDocuments>
    ::Out<RfxDocsContainsRfxDoc>
    RETURN result::!{ id, label }

// --- Proposal → Content → Document ---
// Used by: proposal.content, proposal.chat, proposal.scoring.fixed

#[mcp]
QUERY GetProposalDocuments(projectId: String) =>
    result <- N<Project>({externalId: projectId})
    ::Out<ProjectHasProposal>
    ::Out<ProposalHasContent>
    ::Out<ContentContainsDocument>
    RETURN result::!{ id, label }

// --- Proposal → Insights → Insight ---
// Used by: proposal.content, proposal.chat, proposal.insights.fixed

#[mcp]
QUERY GetProposalInsights(projectId: String) =>
    result <- N<Project>({externalId: projectId})
    ::Out<ProjectHasProposal>
    ::Out<ProposalHasInsights>
    ::Out<InsightsHasInsight>
    RETURN result::!{ id, label }

// --- Project → ProjectManager ---
// Used by: proposal.evaluation, chat.project, project.suitability, all fixed variants

#[mcp]
QUERY GetProjectPM(projectId: String) =>
    result <- N<Project>({externalId: projectId})::Out<LedBy>
    RETURN result::!{ id, label }

// --- Project → Location ---
// Used by: proposal.evaluation, chat.project, project.suitability, all fixed variants

#[mcp]
QUERY GetProjectLocation(projectId: String) =>
    result <- N<Project>({externalId: projectId})::Out<ConductedAt>
    RETURN result::!{ id, label }

// --- Project → AISuitability ---
// Used by: chat.project, project.suitability, chat.project.fixed

#[mcp]
QUERY GetProjectSuitability(projectId: String) =>
    result <- N<Project>({externalId: projectId})::Out<HasSuitability>
    RETURN result::!{ id, label }

// --- Project → TeamRecommendation ---
// Used by: chat.project, project.suitability, chat.project.fixed

#[mcp]
QUERY GetProjectRecommendation(projectId: String) =>
    result <- N<Project>({externalId: projectId})::Out<HasRecommendation>
    RETURN result::!{ id, label }

// --- Project → File (direct project files) ---
// Used by: chat.project, project.suitability

#[mcp]
QUERY GetProjectFiles(projectId: String, start: U32, end: U32) =>
    result <- N<Project>({externalId: projectId})::Out<HasFile>
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- Project → PursuitTeamMember ---
// Used by: proposal.evaluation, proposal.chat, chat.project, all fixed variants

#[mcp]
QUERY GetProjectTeamMembers(projectId: String, start: U32, end: U32) =>
    result <- N<Project>({externalId: projectId})::Out<HasTeamMember>
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- PursuitTeamMember → User ---
// Used by: all templates that traverse team members to get user details

#[mcp]
QUERY GetTeamMemberUser(teamMemberId: String) =>
    result <- N<PursuitTeamMember>({externalId: teamMemberId})::Out<IsUser>
    RETURN result::!{ id, label }

// --- PursuitTeamMember → PursuitTask ---
// Used by: pursuit.suitability.fixed, proposal.chat.fixed, chat.project.fixed

#[mcp]
QUERY GetTeamMemberTasks(teamMemberId: String) =>
    result <- N<PursuitTeamMember>({externalId: teamMemberId})::Out<HasTask>
    RETURN result::!{ id, label }

// --- Project → PursuitLocation ---
// Used by: pursuit.suitability.fixed, proposal.chat.fixed, chat.project.fixed

#[mcp]
QUERY GetProjectPursuitLocations(projectId: String) =>
    result <- N<Project>({externalId: projectId})::Out<HasPursuitLocation>
    RETURN result::!{ id, label }

// --- Project → Client (with role) ---
// Used by: proposal.evaluation.section, proposal.*.fixed, chat.project.fixed

#[mcp]
QUERY GetProjectClients(projectId: String) =>
    result <- N<Project>({externalId: projectId})::Out<HasClient>
    RETURN result::!{ id, label }

// --- User credentials ---
// Used by: staff.user, proposal.evaluation.section, proposal.content.fixed,
//          pursuit.suitability.fixed, proposal.chat.fixed, chat.project.fixed

#[mcp]
QUERY GetUserEducation(userId: String) =>
    result <- N<User>({externalId: userId})::Out<Attended>
    RETURN result::!{ id, label }

#[mcp]
QUERY GetUserCertifications(userId: String) =>
    result <- N<User>({externalId: userId})::Out<Completed>
    RETURN result::!{ id, label }

#[mcp]
QUERY GetUserRegistrations(userId: String) =>
    result <- N<User>({externalId: userId})::Out<UserHasRegistration>
    RETURN result::!{ id, label }

#[mcp]
QUERY GetUserExperience(userId: String) =>
    result <- N<User>({externalId: userId})::Out<WorkedOn>
    RETURN result::!{ id, label }

#[mcp]
QUERY GetUserExpertise(userId: String) =>
    result <- N<User>({externalId: userId})::Out<UserHasExpertise>
    RETURN result::!{ id, label }

// --- Client details ---
// Used by: client.detail, client.all, chat.client, chat.client.fixed

#[mcp]
QUERY GetClientLocations(clientId: String) =>
    result <- N<Client>({externalId: clientId})::Out<LocatedAt>
    RETURN result::!{ id, label }

#[mcp]
QUERY GetClientContacts(clientId: String) =>
    result <- N<Client>({externalId: clientId})::Out<ClientHasContact>
    RETURN result::!{ id, label }

#[mcp]
QUERY GetClientKeyRoles(clientId: String) =>
    result <- N<Client>({externalId: clientId})::Out<ClientHasKeyRole>
    RETURN result::!{ id, label }

// --- ClientKeyRole → ClientContact (FILLED_BY) ---
// Used by: client.detail, chat.client.fixed, proposal.evaluation.section

#[mcp]
QUERY GetKeyRoleContact(keyRoleId: String) =>
    result <- N<ClientKeyRole>({externalId: keyRoleId})::Out<FilledBy>
    RETURN result::!{ id, label }

// --- Client → parent Client ---
// Used by: client.detail, chat.client.fixed

#[mcp]
QUERY GetClientParent(clientId: String) =>
    result <- N<Client>({externalId: clientId})::Out<ChildOf>
    RETURN result::!{ id, label }

// --- Client ← Project (reverse: projects for a client) ---
// Used by: client.detail, chat.client

#[mcp]
QUERY GetClientProjects(clientId: String, start: U32, end: U32) =>
    result <- N<Client>({externalId: clientId})::In<HasClient>
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- File system traversal (direct children only — app does recursion) ---
// Used by: files.folder, files.directory.chat.fixed

#[mcp]
QUERY GetFolderDirectFiles(folderId: String, start: U32, end: U32) =>
    result <- N<Folder>({externalId: folderId})::Out<FolderContainsFile>
    ::RANGE(start, end)
    RETURN result::!{ id, label }

#[mcp]
QUERY GetFolderDirectSubfolders(folderId: String, start: U32, end: U32) =>
    result <- N<Folder>({externalId: folderId})::Out<FolderContainsFolder>
    ::RANGE(start, end)
    RETURN result::!{ id, label }

#[mcp]
QUERY GetFilesRootFolders(filesRootId: String, start: U32, end: U32) =>
    result <- N<FilesRoot>({externalId: filesRootId})::Out<FilesRootContainsFolder>
    ::RANGE(start, end)
    RETURN result::!{ id, label }

#[mcp]
QUERY GetFilesRootFiles(filesRootId: String, start: U32, end: U32) =>
    result <- N<FilesRoot>({externalId: filesRootId})::Out<FilesRootContainsFile>
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// =============================================================================
// SECTION 6: FILTERED COLLECTIONS (list queries with WHERE filters)
//
// Same externalId input + ::!{ id, label } exclusion as Sections 4-5.
// Queries that filter by stored properties (firmId on File/DocumentChunk) use
// String params for those WHERE clauses since they match Postgres UUIDs stored
// during ingestion.
// =============================================================================

// --- Firm → all users (staff.all, staff.all.fixed) ---

#[mcp]
QUERY GetFirmUsers(firmId: String, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasUser>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- Firm → all projects (chat.general) ---

#[mcp]
QUERY GetFirmProjects(firmId: String, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- Firm → active pursuits (chat.pursuits) ---

#[mcp]
QUERY GetFirmPursuits(firmId: String, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::WHERE(_::{entityClassification}::EQ("PURSUIT"))
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- Firm → all clients (client.all) ---

#[mcp]
QUERY GetFirmClients(firmId: String, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasClient>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- Firm → FilesRoot ---

#[mcp]
QUERY GetFirmFilesRoot(firmId: String) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasFilesRoot>
    RETURN result::!{ id, label }

// --- Firm → proposal templates (proposal.outline.with_templates) ---

#[mcp]
QUERY GetFirmProposalTemplates(firmId: String, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProposalTemplate>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- Firm → all indexed files (files.firm — bypasses folder tree traversal) ---
// Uses firmId property on File instead of recursive CONTAINS*1..6

#[mcp]
QUERY GetFirmIndexedFiles(firmId: String, start: U32, end: U32) =>
    result <- N<File>::WHERE(_::{firmId}::EQ(firmId))
    ::WHERE(_::{vectorDocumentId}::NEQ(""))
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- Find file by path (files.document alternative lookup) ---

#[mcp]
QUERY GetFilesByFirmAndPath(firmId: String, folderPath: String, start: U32, end: U32) =>
    result <- N<File>::WHERE(_::{firmId}::EQ(firmId))
    ::WHERE(_::{folderPath}::EQ(folderPath))
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- Users by expertise (staff.byExpertise) ---
// App layer: get all AreaOfExpertise matching name, then traverse In<UserHasExpertise>

#[mcp]
QUERY GetExpertiseByName(expertiseName: String) =>
    result <- N<AreaOfExpertise>::WHERE(_::{name}::EQ(expertiseName))
    RETURN result::!{ id, label }

#[mcp]
QUERY GetUsersWithExpertise(expertiseId: String, start: U32, end: U32) =>
    result <- N<AreaOfExpertise>({externalId: expertiseId})::In<UserHasExpertise>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- Contact → projects assigned (client.project, client.detail) ---

#[mcp]
QUERY GetContactAssignedProjects(contactId: String, start: U32, end: U32) =>
    result <- N<ClientContact>({externalId: contactId})::Out<ContactAssignedToProject>
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- Project → ClientContact (HAS_CLIENT_CONTACT from ProjectClientStaffMember) ---

QUERY LinkProjectClientContact(projectId: String, contactId: String, role: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<ClientContact>({externalId: contactId})
    result <- AddE<ProjectHasClientContact>({role: role})::From(from)::To(to)
    RETURN result

#[mcp]
QUERY GetProjectClientContacts(projectId: String, start: U32, end: U32) =>
    result <- N<Project>({externalId: projectId})::Out<ProjectHasClientContact>
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- Project → ProposalTemplate (DERIVED_TEMPLATE via sourceProjectId) ---

QUERY LinkProjectDerivedTemplate(projectId: String, templateId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<ProposalTemplate>({externalId: templateId})
    result <- AddE<DerivedTemplate>::From(from)::To(to)
    RETURN result

// --- Project → ProposalTemplate (derived templates read query) ---

#[mcp]
QUERY GetProjectDerivedTemplates(projectId: String, start: U32, end: U32) =>
    result <- N<Project>({externalId: projectId})::Out<DerivedTemplate>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- User → Projects (via PursuitTeamMember → Project) ---
// Used by: staff.user, chat context requiring user's project assignments

#[mcp]
QUERY GetUserTeamMemberships(userId: String, start: U32, end: U32) =>
    result <- N<User>({externalId: userId})::Out<UserAssignedToTeamMember>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- User → Projects (direct assignment via WorksOnProject) ---

#[mcp]
QUERY GetUserDirectProjects(userId: String, start: U32, end: U32) =>
    result <- N<User>({externalId: userId})::Out<WorksOnProject>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- Firm → Proposal templates filtered by endMarkets (proposal.outline.with_templates) ---
// endMarkets is stored as a comma-separated string; app layer does substring matching
// For exact match use this query; for partial match, app layer filters GetFirmProposalTemplates

#[mcp]
QUERY GetFirmProposalTemplatesByEndMarket(firmId: String, endMarket: String, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProposalTemplate>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::WHERE(_::{endMarkets}::CONTAINS(endMarket))
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// --- Firm → Vendors ---

#[mcp]
QUERY GetFirmVendors(firmId: String, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasVendor>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::!{ id, label }

// =============================================================================
// SECTION 6B: ANALYTICAL QUERIES (sorting, filtering, aggregation)
//
// Purpose-built queries for common analytical questions from AI agents.
// These use WHERE + ORDER + RANGE to return small, targeted result sets
// and property selection (::{field1, field2}) to minimize token consumption.
// =============================================================================

// --- Top N by Budget / Fee ---

// Top N pursuits by budget for a firm (e.g. "What are Alta's 3 highest-budget pursuits?")
#[mcp]
QUERY GetFirmTopPursuitsByBudget(firmId: String, limit: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{pursuitStatus}::NEQ(""))
    ::WHERE(_::{deletedAt}::EQ(""))
    ::ORDER<Desc>(_::{budget})
    ::RANGE(0, limit)
    RETURN result::{externalId, name, budget, estimatedFee, location, pursuitStatus, pursuitPriority}

// Top N projects by budget (e.g. "Our biggest projects")
#[mcp]
QUERY GetFirmTopProjectsByBudget(firmId: String, limit: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::ORDER<Desc>(_::{budget})
    ::RANGE(0, limit)
    RETURN result::{externalId, name, budget, estimatedFee, location, status, clientName}

// Top N projects by estimated fee
#[mcp]
QUERY GetFirmTopProjectsByFee(firmId: String, limit: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::ORDER<Desc>(_::{estimatedFee})
    ::RANGE(0, limit)
    RETURN result::{externalId, name, budget, estimatedFee, expectedRevenue, location, status}

// --- Location Filtering ---

// Projects matching a location substring (e.g. "Birmingham", "California", "CA")
#[mcp]
QUERY GetFirmProjectsByLocation(firmId: String, location: String, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{location}::CONTAINS(location))
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::{externalId, name, budget, estimatedFee, location, status, pursuitStatus, clientName}

// --- Status Filtering ---

// Pursuits filtered by pursuit status (e.g. "GO", "NO_GO", "EVALUATING")
#[mcp]
QUERY GetFirmPursuitsByStatus(firmId: String, pursuitStatus: String, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{pursuitStatus}::EQ(pursuitStatus))
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::{externalId, name, budget, estimatedFee, location, pursuitStatus, pursuitPriority, bidDate}

// Projects filtered by project status (e.g. "active", "completed", "on_hold")
#[mcp]
QUERY GetFirmProjectsByStatus(firmId: String, status: String, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{status}::EQ(status))
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::{externalId, name, budget, estimatedFee, location, status, clientName, startDate, endDate}

// --- Budget Threshold ---

// Projects with budget above a minimum (e.g. "Projects over $1M")
#[mcp]
QUERY GetFirmProjectsByMinBudget(firmId: String, minBudget: F64, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{budget}::GTE(minBudget))
    ::WHERE(_::{deletedAt}::EQ(""))
    ::ORDER<Desc>(_::{budget})
    ::RANGE(start, end)
    RETURN result::{externalId, name, budget, estimatedFee, location, status, clientName}

// --- Count / Aggregation Queries (minimal tokens) ---

// Count of projects grouped by status
#[mcp]
QUERY GetFirmProjectCountByStatus(firmId: String) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{deletedAt}::EQ(""))
    RETURN result::GROUP_BY(status)

// Count of pursuits grouped by pursuit status
#[mcp]
QUERY GetFirmPursuitCountByStatus(firmId: String) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{pursuitStatus}::NEQ(""))
    ::WHERE(_::{deletedAt}::EQ(""))
    RETURN result::GROUP_BY(pursuitStatus)

// Count of projects grouped by service line
#[mcp]
QUERY GetFirmProjectCountByServiceLine(firmId: String) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{deletedAt}::EQ(""))
    RETURN result::GROUP_BY(serviceLines)

// Total project count for a firm
// NOTE: Not #[mcp] — COUNT returns a scalar which the MCP macro can't serialize as a struct.
QUERY GetFirmProjectCount(firmId: String) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::COUNT
    RETURN result

// Total user count for a firm
// NOTE: Not #[mcp] — COUNT returns a scalar which the MCP macro can't serialize as a struct.
QUERY GetFirmUserCount(firmId: String) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasUser>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::COUNT
    RETURN result

// --- Summary Queries (compact field selection for large collections) ---
// --- Summary Queries (compact field selection for large collections) ---

// Compact project listing — 6 key fields instead of 48
#[mcp]
QUERY GetFirmProjectsSummary(firmId: String, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasProject>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::{externalId, name, budget, status, location, pursuitStatus}

// Compact user listing — 6 key fields instead of 19
#[mcp]
QUERY GetFirmUsersSummary(firmId: String, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasUser>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::{externalId, firstName, lastName, jobTitle, email, category}

// Compact client listing — 5 key fields instead of 18
#[mcp]
QUERY GetFirmClientsSummary(firmId: String, start: U32, end: U32) =>
    result <- N<Firm>({externalId: firmId})::Out<FirmHasClient>
    ::WHERE(_::{deletedAt}::EQ(""))
    ::RANGE(start, end)
    RETURN result::{externalId, name, clientType, market, status}

// =============================================================================
// SECTION 7: CDC NODE UPSERT + BATCH UPSERT (idempotent seed + CDC)
// =============================================================================

QUERY UpsertFirm(
    externalId: String, name: String, description: String,
    domain: String, email: String, phone: String,
    city: String, state: String,
    createdAt: String, deletedAt: String
) =>
    existing <- N<Firm>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, name: name, description: description,
        domain: domain, email: email, phone: phone,
        city: city, state: state,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY UpsertUser(
    externalId: String, firmId: String, firstName: String, lastName: String,
    email: String, phone: String, designation: String, jobTitle: String,
    category: String, accountType: String, admin: Boolean,
    areasOfExpertise: String, overview: String, yearsOfExperience: I64,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<User>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, firmId: firmId, firstName: firstName, lastName: lastName,
        email: email, phone: phone, designation: designation, jobTitle: jobTitle,
        category: category, accountType: accountType, admin: admin,
        areasOfExpertise: areasOfExpertise, overview: overview,
        yearsOfExperience: yearsOfExperience,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertProject(
    externalId: String, firmId: String, clientId: String, billingClientId: String,
    name: String, description: String, referenceNumber: String,
    location: String, clientName: String,
    projectManagerName: String, projectManagerId: String,
    budget: F64, startDate: String, endDate: String, status: String,
    entityClassification: String, createdAt: String, deletedAt: String,
    suitabilityDecision: String, suitabilityConfidenceScore: F64,
    suitabilityReason: String, suitabilityGeneratedAt: String,
    recommendationDecision: String, recommendationConfidenceScore: F64,
    pursuitStatus: String, pursuitPriority: String, pursuitArchived: Boolean,
    serviceLines: String, endMarkets: String,
    opportunitySourceCategory: String, opportunitySourceDetails: String,
    responsibility: String, opportunityUrl: String, opportunityDescription: String,
    estimatedFee: F64, estimatedConstructionCost: F64, expectedRevenue: F64,
    announcementDate: String, rfpIssueDate: String, submissionsOpenDate: String,
    questionsDueDate: String, bidDate: String, interviewDate: String,
    awardAnnouncementDate: String, noticeToProceedDate: String,
    expectedStartDate: String, expectedEndDate: String
) =>
    existing <- N<Project>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, firmId: firmId, clientId: clientId, billingClientId: billingClientId,
        name: name, description: description, referenceNumber: referenceNumber,
        location: location, clientName: clientName,
        projectManagerName: projectManagerName, projectManagerId: projectManagerId,
        budget: budget, startDate: startDate, endDate: endDate, status: status,
        entityClassification: entityClassification, createdAt: createdAt, deletedAt: deletedAt,
        suitabilityDecision: suitabilityDecision,
        suitabilityConfidenceScore: suitabilityConfidenceScore,
        suitabilityReason: suitabilityReason,
        suitabilityGeneratedAt: suitabilityGeneratedAt,
        recommendationDecision: recommendationDecision,
        recommendationConfidenceScore: recommendationConfidenceScore,
        pursuitStatus: pursuitStatus, pursuitPriority: pursuitPriority,
        pursuitArchived: pursuitArchived,
        serviceLines: serviceLines, endMarkets: endMarkets,
        opportunitySourceCategory: opportunitySourceCategory,
        opportunitySourceDetails: opportunitySourceDetails,
        responsibility: responsibility, opportunityUrl: opportunityUrl,
        opportunityDescription: opportunityDescription,
        estimatedFee: estimatedFee, estimatedConstructionCost: estimatedConstructionCost,
        expectedRevenue: expectedRevenue,
        announcementDate: announcementDate, rfpIssueDate: rfpIssueDate,
        submissionsOpenDate: submissionsOpenDate, questionsDueDate: questionsDueDate,
        bidDate: bidDate, interviewDate: interviewDate,
        awardAnnouncementDate: awardAnnouncementDate,
        noticeToProceedDate: noticeToProceedDate,
        expectedStartDate: expectedStartDate, expectedEndDate: expectedEndDate
    })
    RETURN result

QUERY UpsertVendor(
    externalId: String, firmId: String, name: String, description: String,
    createdAt: String, deletedAt: String
) =>
    existing <- N<Vendor>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, firmId: firmId, name: name, description: description,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY UpsertClient(
    externalId: String, firmId: String, name: String, referenceNumber: String,
    description: String, website: String, email: String, phone: String,
    status: String, clientType: String, clientSubType: String,
    market: String, governmentAgency: String, parentClientId: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<Client>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, firmId: firmId, name: name, referenceNumber: referenceNumber,
        description: description, website: website, email: email, phone: phone,
        status: status, clientType: clientType, clientSubType: clientSubType,
        market: market, governmentAgency: governmentAgency,
        parentClientId: parentClientId,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertFolder(
    externalId: String, firmId: String, parentId: String,
    name: String, path: String,
    createdAt: String, deletedAt: String
) =>
    existing <- N<Folder>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, firmId: firmId, parentId: parentId,
        name: name, path: path,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY UpsertFile(
    externalId: String, firmId: String, folderId: String, folderPath: String,
    name: String, path: String, gcsPath: String, mimeType: String,
    sizeBytes: I64, projectId: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<File>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, firmId: firmId, folderId: folderId, folderPath: folderPath,
        name: name, path: path, gcsPath: gcsPath, mimeType: mimeType,
        sizeBytes: sizeBytes, projectId: projectId,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertProposal(
    externalId: String, projectId: String, firmId: String,
    createdAt: String, deletedAt: String
) =>
    existing <- N<Proposal>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, projectId: projectId, firmId: firmId,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY UpsertEvaluation(
    externalId: String, projectId: String, firmId: String,
    sectionsGcsBasePath: String, createdAt: String, deletedAt: String
) =>
    existing <- N<Evaluation>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, projectId: projectId, firmId: firmId,
        sectionsGcsBasePath: sectionsGcsBasePath,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY UpsertRfxDocument(
    externalId: String, projectId: String, firmId: String,
    name: String, path: String, gcsPath: String,
    contentType: String, sizeBytes: I64,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<RfxDocument>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, projectId: projectId, firmId: firmId,
        name: name, path: path, gcsPath: gcsPath,
        contentType: contentType, sizeBytes: sizeBytes,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertDocument(
    externalId: String, documentSetId: String, projectId: String, firmId: String,
    name: String, gcsPath: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<Document>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, documentSetId: documentSetId, projectId: projectId, firmId: firmId,
        name: name, gcsPath: gcsPath,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertInsights(
    externalId: String, projectId: String, firmId: String,
    insightsGcsBasePath: String, createdAt: String, deletedAt: String
) =>
    existing <- N<Insights>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, projectId: projectId, firmId: firmId,
        insightsGcsBasePath: insightsGcsBasePath,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY UpsertInsight(
    externalId: String, insightsId: String, name: String,
    gcsPath: String, projectId: String, firmId: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<Insight>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, insightsId: insightsId, name: name,
        gcsPath: gcsPath, projectId: projectId, firmId: firmId,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertEducation(
    externalId: String, userId: String, firmId: String,
    degree: String, major: String, university: String,
    graduationYear: I64, createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<Education>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, userId: userId, firmId: firmId,
        degree: degree, major: major, university: university,
        graduationYear: graduationYear, createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertCertification(
    externalId: String, userId: String, firmId: String,
    name: String, description: String, issuingOrg: String,
    dateObtained: String, expirationDate: String, issuedAt: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<Certification>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, userId: userId, firmId: firmId,
        name: name, description: description, issuingOrg: issuingOrg,
        dateObtained: dateObtained, expirationDate: expirationDate,
        issuedAt: issuedAt, createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertRegistration(
    externalId: String, userId: String, firmId: String,
    name: String, issuedAt: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<Registration>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, userId: userId, firmId: firmId,
        name: name, issuedAt: issuedAt,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertProjectExperience(
    externalId: String, userId: String, firmId: String,
    name: String, description: String, role: String, clientName: String,
    startDate: String, endDate: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<ProjectExperience>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, userId: userId, firmId: firmId,
        name: name, description: description, role: role, clientName: clientName,
        startDate: startDate, endDate: endDate,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertClientContact(
    externalId: String, clientId: String, firmId: String,
    firstName: String, lastName: String, jobTitle: String,
    email: String, phone: String, linkedinUrl: String, notes: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<ClientContact>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, clientId: clientId, firmId: firmId,
        firstName: firstName, lastName: lastName, jobTitle: jobTitle,
        email: email, phone: phone, linkedinUrl: linkedinUrl, notes: notes,
        createdAt: createdAt, deletedAt: deletedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertClientLocation(
    externalId: String, clientId: String, firmId: String,
    name: String, address: String, city: String, state: String,
    zip: String, country: String, email: String, phone: String,
    isBillingAddress: Boolean, isPrimaryAddress: Boolean,
    additionalInformation: String,
    createdAt: String, deletedAt: String
) =>
    existing <- N<ClientLocation>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, clientId: clientId, firmId: firmId,
        name: name, address: address, city: city, state: state,
        zip: zip, country: country, email: email, phone: phone,
        isBillingAddress: isBillingAddress, isPrimaryAddress: isPrimaryAddress,
        additionalInformation: additionalInformation,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY UpsertClientKeyRole(
    externalId: String, clientId: String, firmId: String,
    role: String, clientStaffMemberId: String,
    createdAt: String, deletedAt: String
) =>
    existing <- N<ClientKeyRole>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, clientId: clientId, firmId: firmId,
        role: role, clientStaffMemberId: clientStaffMemberId,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY UpsertPursuitTeamMember(
    externalId: String, firmId: String, projectId: String, userId: String,
    role: String, roleType: String, startDate: String, endDate: String,
    createdAt: String, deletedAt: String
) =>
    existing <- N<PursuitTeamMember>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, firmId: firmId, projectId: projectId, userId: userId,
        role: role, roleType: roleType, startDate: startDate, endDate: endDate,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY UpsertPursuitTask(
    externalId: String, firmId: String, projectId: String, teamMemberId: String,
    description: String, status: String,
    creatorId: String, assigneeId: String, comments: String,
    createdAt: String, deletedAt: String
) =>
    existing <- N<PursuitTask>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, firmId: firmId, projectId: projectId, teamMemberId: teamMemberId,
        description: description, status: status,
        creatorId: creatorId, assigneeId: assigneeId, comments: comments,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY UpsertPursuitLocation(
    externalId: String, projectId: String, firmId: String,
    name: String, address: String, city: String, state: String,
    zip: String, country: String, email: String, phone: String,
    additionalInformation: String,
    createdAt: String, deletedAt: String
) =>
    existing <- N<PursuitLocation>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, projectId: projectId, firmId: firmId,
        name: name, address: address, city: city, state: state,
        zip: zip, country: country, email: email, phone: phone,
        additionalInformation: additionalInformation,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY UpsertLocation(
    externalId: String, projectId: String, firmId: String,
    name: String, address: String, city: String, state: String,
    zip: String, country: String, email: String, phone: String,
    createdAt: String, deletedAt: String
) =>
    existing <- N<Location>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, projectId: projectId, firmId: firmId,
        name: name, address: address, city: city, state: state,
        zip: zip, country: country, email: email, phone: phone,
        createdAt: createdAt, deletedAt: deletedAt
    })
    RETURN result

QUERY UpsertAISuitability(
    externalId: String, projectId: String, firmId: String,
    decision: String, confidenceScore: F64, reason: String,
    citationsPath: String, generatedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<AISuitability>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, projectId: projectId, firmId: firmId,
        decision: decision, confidenceScore: confidenceScore, reason: reason,
        citationsPath: citationsPath, generatedAt: generatedAt,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertTeamRecommendation(
    externalId: String, projectId: String, firmId: String,
    decision: String, confidenceScore: F64, reason: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<TeamRecommendation>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, projectId: projectId, firmId: firmId,
        decision: decision, confidenceScore: confidenceScore, reason: reason,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertProjectManager(
    externalId: String, userId: String,
    firstName: String, lastName: String, email: String,
    jobTitle: String, designation: String,
    overview: String, areasOfExpertise: String
) =>
    existing <- N<ProjectManager>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, userId: userId,
        firstName: firstName, lastName: lastName, email: email,
        jobTitle: jobTitle, designation: designation,
        overview: overview, areasOfExpertise: areasOfExpertise
    })
    RETURN result

QUERY UpsertAreaOfExpertise(externalId: String, name: String) =>
    existing <- N<AreaOfExpertise>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({externalId: externalId, name: name})
    RETURN result

QUERY UpsertEvaluationSection(
    externalId: String, evaluationId: String, name: String,
    gcsPath: String, projectId: String, firmId: String,
    vectorDocumentId: String, vectorStoreStatus: String
) =>
    existing <- N<EvaluationSection>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, evaluationId: evaluationId, name: name,
        gcsPath: gcsPath, projectId: projectId, firmId: firmId,
        vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
    })
    RETURN result

QUERY UpsertRfxDocumentsContainer(externalId: String, projectId: String, firmId: String) =>
    existing <- N<RfxDocuments>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({externalId: externalId, projectId: projectId, firmId: firmId})
    RETURN result

QUERY UpsertContentContainer(externalId: String, projectId: String, firmId: String) =>
    existing <- N<Content>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({externalId: externalId, projectId: projectId, firmId: firmId})
    RETURN result

QUERY UpsertDocumentChunkText(
    externalId: String, documentId: String, firmId: String, projectId: String,
    sourceType: String, sourceId: String, chunkIndex: I64,
    contentPreview: String,
    summary: String, sectionTitle: String, title: String, headingPath: String
) =>
    existing <- N<DocumentChunkText>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, documentId: documentId, firmId: firmId,
        projectId: projectId, sourceType: sourceType, sourceId: sourceId,
        chunkIndex: chunkIndex, contentPreview: contentPreview,
        summary: summary, sectionTitle: sectionTitle, title: title,
        headingPath: headingPath
    })
    RETURN result

QUERY UpsertFilesRoot(externalId: String, firmId: String, name: String) =>
    existing <- N<FilesRoot>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({externalId: externalId, firmId: firmId, name: name})
    RETURN result

QUERY UpsertProposalTemplate(
    externalId: String, firmId: String, name: String, clientName: String,
    gcsPath: String, sectionCount: I64, orgTypeId: String,
    endMarkets: String, deletedAt: String
) =>
    existing <- N<ProposalTemplate>::WHERE(_::{externalId}::EQ(externalId))
    result <- existing::UpsertN({
        externalId: externalId, firmId: firmId, name: name, clientName: clientName,
        gcsPath: gcsPath, sectionCount: sectionCount, orgTypeId: orgTypeId,
        endMarkets: endMarkets, deletedAt: deletedAt
    })
    RETURN result

// --- Batch upsert: Core entity nodes (idempotent) ---

QUERY BatchUpsertUsers(data: [{
    externalId: String, firmId: String, firstName: String, lastName: String,
    email: String, phone: String, designation: String, jobTitle: String,
    category: String, accountType: String, admin: Boolean,
    areasOfExpertise: String, overview: String, yearsOfExperience: I64,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
}]) =>
    FOR { externalId, firmId, firstName, lastName, email, phone, designation,
          jobTitle, category, accountType, admin, areasOfExpertise, overview,
          yearsOfExperience, createdAt, deletedAt, vectorDocumentId,
          vectorStoreStatus } IN data {
        existing <- N<User>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, firmId: firmId, firstName: firstName,
            lastName: lastName, email: email, phone: phone,
            designation: designation, jobTitle: jobTitle,
            category: category, accountType: accountType, admin: admin,
            areasOfExpertise: areasOfExpertise, overview: overview,
            yearsOfExperience: yearsOfExperience,
            createdAt: createdAt, deletedAt: deletedAt,
            vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertProjects(data: [{
    externalId: String, firmId: String, clientId: String, billingClientId: String,
    name: String, description: String, referenceNumber: String,
    location: String, clientName: String,
    projectManagerName: String, projectManagerId: String,
    budget: F64, startDate: String, endDate: String, status: String,
    entityClassification: String, createdAt: String, deletedAt: String,
    suitabilityDecision: String, suitabilityConfidenceScore: F64,
    suitabilityReason: String, suitabilityGeneratedAt: String,
    recommendationDecision: String, recommendationConfidenceScore: F64,
    pursuitStatus: String, pursuitPriority: String, pursuitArchived: Boolean,
    serviceLines: String, endMarkets: String,
    opportunitySourceCategory: String, opportunitySourceDetails: String,
    responsibility: String, opportunityUrl: String, opportunityDescription: String,
    estimatedFee: F64, estimatedConstructionCost: F64, expectedRevenue: F64,
    announcementDate: String, rfpIssueDate: String, submissionsOpenDate: String,
    questionsDueDate: String, bidDate: String, interviewDate: String,
    awardAnnouncementDate: String, noticeToProceedDate: String,
    expectedStartDate: String, expectedEndDate: String
}]) =>
    FOR { externalId, firmId, clientId, billingClientId, name, description,
          referenceNumber, location, clientName, projectManagerName, projectManagerId,
          budget, startDate, endDate, status, entityClassification, createdAt, deletedAt,
          suitabilityDecision, suitabilityConfidenceScore, suitabilityReason,
          suitabilityGeneratedAt, recommendationDecision, recommendationConfidenceScore,
          pursuitStatus, pursuitPriority, pursuitArchived, serviceLines, endMarkets,
          opportunitySourceCategory, opportunitySourceDetails, responsibility,
          opportunityUrl, opportunityDescription, estimatedFee,
          estimatedConstructionCost, expectedRevenue, announcementDate, rfpIssueDate,
          submissionsOpenDate, questionsDueDate, bidDate, interviewDate,
          awardAnnouncementDate, noticeToProceedDate,
          expectedStartDate, expectedEndDate } IN data {
        existing <- N<Project>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, firmId: firmId, clientId: clientId,
            billingClientId: billingClientId, name: name, description: description,
            referenceNumber: referenceNumber, location: location,
            clientName: clientName, projectManagerName: projectManagerName,
            projectManagerId: projectManagerId, budget: budget,
            startDate: startDate, endDate: endDate, status: status,
            entityClassification: entityClassification,
            createdAt: createdAt, deletedAt: deletedAt,
            suitabilityDecision: suitabilityDecision,
            suitabilityConfidenceScore: suitabilityConfidenceScore,
            suitabilityReason: suitabilityReason,
            suitabilityGeneratedAt: suitabilityGeneratedAt,
            recommendationDecision: recommendationDecision,
            recommendationConfidenceScore: recommendationConfidenceScore,
            pursuitStatus: pursuitStatus, pursuitPriority: pursuitPriority,
            pursuitArchived: pursuitArchived, serviceLines: serviceLines,
            endMarkets: endMarkets,
            opportunitySourceCategory: opportunitySourceCategory,
            opportunitySourceDetails: opportunitySourceDetails,
            responsibility: responsibility, opportunityUrl: opportunityUrl,
            opportunityDescription: opportunityDescription,
            estimatedFee: estimatedFee,
            estimatedConstructionCost: estimatedConstructionCost,
            expectedRevenue: expectedRevenue,
            announcementDate: announcementDate, rfpIssueDate: rfpIssueDate,
            submissionsOpenDate: submissionsOpenDate,
            questionsDueDate: questionsDueDate, bidDate: bidDate,
            interviewDate: interviewDate,
            awardAnnouncementDate: awardAnnouncementDate,
            noticeToProceedDate: noticeToProceedDate,
            expectedStartDate: expectedStartDate, expectedEndDate: expectedEndDate
        })
    }
    RETURN "Success"

QUERY BatchUpsertClients(data: [{
    externalId: String, firmId: String, name: String, referenceNumber: String,
    description: String, website: String, email: String, phone: String,
    status: String, clientType: String, clientSubType: String,
    market: String, governmentAgency: String, parentClientId: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
}]) =>
    FOR { externalId, firmId, name, referenceNumber, description, website,
          email, phone, status, clientType, clientSubType, market,
          governmentAgency, parentClientId, createdAt, deletedAt,
          vectorDocumentId, vectorStoreStatus } IN data {
        existing <- N<Client>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, firmId: firmId, name: name,
            referenceNumber: referenceNumber, description: description,
            website: website, email: email, phone: phone,
            status: status, clientType: clientType, clientSubType: clientSubType,
            market: market, governmentAgency: governmentAgency,
            parentClientId: parentClientId,
            createdAt: createdAt, deletedAt: deletedAt,
            vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertFiles(data: [{
    externalId: String, firmId: String, folderId: String, folderPath: String,
    name: String, path: String, gcsPath: String, mimeType: String,
    sizeBytes: I64, projectId: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
}]) =>
    FOR { externalId, firmId, folderId, folderPath, name, path, gcsPath,
          mimeType, sizeBytes, projectId, createdAt, deletedAt,
          vectorDocumentId, vectorStoreStatus } IN data {
        existing <- N<File>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, firmId: firmId, folderId: folderId,
            folderPath: folderPath, name: name, path: path,
            gcsPath: gcsPath, mimeType: mimeType, sizeBytes: sizeBytes,
            projectId: projectId, createdAt: createdAt, deletedAt: deletedAt,
            vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertFolders(data: [{
    externalId: String, firmId: String, parentId: String,
    name: String, path: String,
    createdAt: String, deletedAt: String
}]) =>
    FOR { externalId, firmId, parentId, name, path, createdAt,
          deletedAt } IN data {
        existing <- N<Folder>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, firmId: firmId, parentId: parentId,
            name: name, path: path,
            createdAt: createdAt, deletedAt: deletedAt
        })
    }
    RETURN "Success"

QUERY BatchUpsertProjectExperiences(data: [{
    externalId: String, userId: String, firmId: String,
    name: String, description: String, role: String, clientName: String,
    startDate: String, endDate: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
}]) =>
    FOR { externalId, userId, firmId, name, description, role, clientName,
          startDate, endDate, createdAt, deletedAt,
          vectorDocumentId, vectorStoreStatus } IN data {
        existing <- N<ProjectExperience>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, userId: userId, firmId: firmId,
            name: name, description: description, role: role, clientName: clientName,
            startDate: startDate, endDate: endDate,
            createdAt: createdAt, deletedAt: deletedAt,
            vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertRfxDocuments(data: [{
    externalId: String, projectId: String, firmId: String,
    name: String, path: String, gcsPath: String,
    contentType: String, sizeBytes: I64,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
}]) =>
    FOR { externalId, projectId, firmId, name, path, gcsPath,
          contentType, sizeBytes, createdAt, deletedAt,
          vectorDocumentId, vectorStoreStatus } IN data {
        existing <- N<RfxDocument>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, projectId: projectId, firmId: firmId,
            name: name, path: path, gcsPath: gcsPath,
            contentType: contentType, sizeBytes: sizeBytes,
            createdAt: createdAt, deletedAt: deletedAt,
            vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertEducations(data: [{
    externalId: String, userId: String, firmId: String,
    degree: String, major: String, university: String,
    graduationYear: I64, createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
}]) =>
    FOR { externalId, userId, firmId, degree, major, university,
          graduationYear, createdAt, deletedAt,
          vectorDocumentId, vectorStoreStatus } IN data {
        existing <- N<Education>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, userId: userId, firmId: firmId,
            degree: degree, major: major, university: university,
            graduationYear: graduationYear, createdAt: createdAt, deletedAt: deletedAt,
            vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertCertifications(data: [{
    externalId: String, userId: String, firmId: String,
    name: String, description: String, issuingOrg: String,
    dateObtained: String, expirationDate: String, issuedAt: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
}]) =>
    FOR { externalId, userId, firmId, name, description, issuingOrg,
          dateObtained, expirationDate, issuedAt, createdAt, deletedAt,
          vectorDocumentId, vectorStoreStatus } IN data {
        existing <- N<Certification>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, userId: userId, firmId: firmId,
            name: name, description: description, issuingOrg: issuingOrg,
            dateObtained: dateObtained, expirationDate: expirationDate,
            issuedAt: issuedAt, createdAt: createdAt, deletedAt: deletedAt,
            vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertRegistrations(data: [{
    externalId: String, userId: String, firmId: String,
    name: String, issuedAt: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
}]) =>
    FOR { externalId, userId, firmId, name, issuedAt,
          createdAt, deletedAt,
          vectorDocumentId, vectorStoreStatus } IN data {
        existing <- N<Registration>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, userId: userId, firmId: firmId,
            name: name, issuedAt: issuedAt,
            createdAt: createdAt, deletedAt: deletedAt,
            vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertClientContacts(data: [{
    externalId: String, clientId: String, firmId: String,
    firstName: String, lastName: String, jobTitle: String,
    email: String, phone: String, linkedinUrl: String, notes: String,
    createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
}]) =>
    FOR { externalId, clientId, firmId, firstName, lastName, jobTitle,
          email, phone, linkedinUrl, notes, createdAt, deletedAt,
          vectorDocumentId, vectorStoreStatus } IN data {
        existing <- N<ClientContact>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, clientId: clientId, firmId: firmId,
            firstName: firstName, lastName: lastName, jobTitle: jobTitle,
            email: email, phone: phone, linkedinUrl: linkedinUrl, notes: notes,
            createdAt: createdAt, deletedAt: deletedAt,
            vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertPursuitTeamMembers(data: [{
    externalId: String, firmId: String, projectId: String, userId: String,
    role: String, roleType: String, startDate: String, endDate: String,
    createdAt: String, deletedAt: String
}]) =>
    FOR { externalId, firmId, projectId, userId, role, roleType,
          startDate, endDate, createdAt, deletedAt } IN data {
        existing <- N<PursuitTeamMember>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, firmId: firmId, projectId: projectId, userId: userId,
            role: role, roleType: roleType, startDate: startDate, endDate: endDate,
            createdAt: createdAt, deletedAt: deletedAt
        })
    }
    RETURN "Success"

QUERY BatchUpsertFirms(data: [{
    externalId: String, name: String, description: String, domain: String,
    email: String, phone: String, city: String, state: String,
    createdAt: String, deletedAt: String
}]) =>
    FOR { externalId, name, description, domain, email, phone, city, state,
          createdAt, deletedAt } IN data {
        existing <- N<Firm>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, name: name, description: description,
            domain: domain, email: email, phone: phone, city: city, state: state,
            createdAt: createdAt, deletedAt: deletedAt
        })
    }
    RETURN "Success"

QUERY BatchUpsertVendors(data: [{
    externalId: String, firmId: String, name: String, description: String,
    createdAt: String, deletedAt: String
}]) =>
    FOR { externalId, firmId, name, description, createdAt, deletedAt } IN data {
        existing <- N<Vendor>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, firmId: firmId, name: name,
            description: description, createdAt: createdAt, deletedAt: deletedAt
        })
    }
    RETURN "Success"

QUERY BatchUpsertProposals(data: [{
    externalId: String, projectId: String, firmId: String,
    createdAt: String, deletedAt: String
}]) =>
    FOR { externalId, projectId, firmId, createdAt, deletedAt } IN data {
        existing <- N<Proposal>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, projectId: projectId, firmId: firmId,
            createdAt: createdAt, deletedAt: deletedAt
        })
    }
    RETURN "Success"

QUERY BatchUpsertEvaluations(data: [{
    externalId: String, projectId: String, firmId: String,
    sectionsGcsBasePath: String, createdAt: String, deletedAt: String
}]) =>
    FOR { externalId, projectId, firmId, sectionsGcsBasePath, createdAt, deletedAt } IN data {
        existing <- N<Evaluation>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, projectId: projectId, firmId: firmId,
            sectionsGcsBasePath: sectionsGcsBasePath, createdAt: createdAt, deletedAt: deletedAt
        })
    }
    RETURN "Success"

QUERY BatchUpsertDocuments(data: [{
    externalId: String, documentSetId: String, projectId: String, firmId: String,
    name: String, gcsPath: String, createdAt: String, deletedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
}]) =>
    FOR { externalId, documentSetId, projectId, firmId, name, gcsPath, createdAt,
          deletedAt, vectorDocumentId, vectorStoreStatus } IN data {
        existing <- N<Document>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, documentSetId: documentSetId, projectId: projectId,
            firmId: firmId, name: name, gcsPath: gcsPath, createdAt: createdAt,
            deletedAt: deletedAt, vectorDocumentId: vectorDocumentId,
            vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertInsightsContainers(data: [{
    externalId: String, projectId: String, firmId: String,
    insightsGcsBasePath: String, createdAt: String, deletedAt: String
}]) =>
    FOR { externalId, projectId, firmId, insightsGcsBasePath, createdAt, deletedAt } IN data {
        existing <- N<Insights>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, projectId: projectId, firmId: firmId,
            insightsGcsBasePath: insightsGcsBasePath, createdAt: createdAt, deletedAt: deletedAt
        })
    }
    RETURN "Success"

QUERY BatchUpsertClientLocations(data: [{
    externalId: String, clientId: String, firmId: String, name: String,
    address: String, city: String, state: String, zip: String, country: String,
    email: String, phone: String, isBillingAddress: Boolean, isPrimaryAddress: Boolean,
    additionalInformation: String, createdAt: String, deletedAt: String
}]) =>
    FOR { externalId, clientId, firmId, name, address, city, state, zip, country,
          email, phone, isBillingAddress, isPrimaryAddress, additionalInformation,
          createdAt, deletedAt } IN data {
        existing <- N<ClientLocation>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, clientId: clientId, firmId: firmId, name: name,
            address: address, city: city, state: state, zip: zip, country: country,
            email: email, phone: phone, isBillingAddress: isBillingAddress,
            isPrimaryAddress: isPrimaryAddress, additionalInformation: additionalInformation,
            createdAt: createdAt, deletedAt: deletedAt
        })
    }
    RETURN "Success"

QUERY BatchUpsertClientKeyRoles(data: [{
    externalId: String, clientId: String, firmId: String, role: String,
    clientStaffMemberId: String, createdAt: String, deletedAt: String
}]) =>
    FOR { externalId, clientId, firmId, role, clientStaffMemberId, createdAt, deletedAt } IN data {
        existing <- N<ClientKeyRole>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, clientId: clientId, firmId: firmId, role: role,
            clientStaffMemberId: clientStaffMemberId, createdAt: createdAt, deletedAt: deletedAt
        })
    }
    RETURN "Success"

QUERY BatchUpsertLocations(data: [{
    externalId: String, projectId: String, firmId: String, name: String,
    address: String, city: String, state: String, zip: String, country: String,
    email: String, phone: String, createdAt: String, deletedAt: String
}]) =>
    FOR { externalId, projectId, firmId, name, address, city, state, zip, country,
          email, phone, createdAt, deletedAt } IN data {
        existing <- N<Location>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, projectId: projectId, firmId: firmId, name: name,
            address: address, city: city, state: state, zip: zip, country: country,
            email: email, phone: phone, createdAt: createdAt, deletedAt: deletedAt
        })
    }
    RETURN "Success"

QUERY BatchUpsertPursuitTasks(data: [{
    externalId: String, firmId: String, projectId: String, teamMemberId: String,
    description: String, status: String, creatorId: String, assigneeId: String,
    comments: String, createdAt: String, deletedAt: String
}]) =>
    FOR { externalId, firmId, projectId, teamMemberId, description, status, creatorId,
          assigneeId, comments, createdAt, deletedAt } IN data {
        existing <- N<PursuitTask>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, firmId: firmId, projectId: projectId,
            teamMemberId: teamMemberId, description: description, status: status,
            creatorId: creatorId, assigneeId: assigneeId, comments: comments,
            createdAt: createdAt, deletedAt: deletedAt
        })
    }
    RETURN "Success"

QUERY BatchUpsertPursuitLocations(data: [{
    externalId: String, projectId: String, firmId: String, name: String,
    address: String, city: String, state: String, zip: String, country: String,
    email: String, phone: String, additionalInformation: String,
    createdAt: String, deletedAt: String
}]) =>
    FOR { externalId, projectId, firmId, name, address, city, state, zip, country,
          email, phone, additionalInformation, createdAt, deletedAt } IN data {
        existing <- N<PursuitLocation>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, projectId: projectId, firmId: firmId, name: name,
            address: address, city: city, state: state, zip: zip, country: country,
            email: email, phone: phone, additionalInformation: additionalInformation,
            createdAt: createdAt, deletedAt: deletedAt
        })
    }
    RETURN "Success"

QUERY BatchUpsertProposalTemplates(data: [{
    externalId: String, firmId: String, name: String, clientName: String,
    gcsPath: String, sectionCount: I64, orgTypeId: String, endMarkets: String,
    deletedAt: String
}]) =>
    FOR { externalId, firmId, name, clientName, gcsPath, sectionCount, orgTypeId,
          endMarkets, deletedAt } IN data {
        existing <- N<ProposalTemplate>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, firmId: firmId, name: name, clientName: clientName,
            gcsPath: gcsPath, sectionCount: sectionCount, orgTypeId: orgTypeId,
            endMarkets: endMarkets, deletedAt: deletedAt
        })
    }
    RETURN "Success"

QUERY BatchUpsertFilesRoots(data: [{
    externalId: String, firmId: String, name: String
}]) =>
    FOR { externalId, firmId, name } IN data {
        existing <- N<FilesRoot>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, firmId: firmId, name: name
        })
    }
    RETURN "Success"

QUERY BatchUpsertAreasOfExpertise(data: [{
    externalId: String, name: String
}]) =>
    FOR { externalId, name } IN data {
        existing <- N<AreaOfExpertise>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, name: name
        })
    }
    RETURN "Success"

QUERY BatchUpsertProjectManagers(data: [{
    externalId: String, userId: String, firstName: String, lastName: String,
    email: String, jobTitle: String, designation: String, overview: String,
    areasOfExpertise: String
}]) =>
    FOR { externalId, userId, firstName, lastName, email, jobTitle, designation,
          overview, areasOfExpertise } IN data {
        existing <- N<ProjectManager>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, userId: userId, firstName: firstName,
            lastName: lastName, email: email, jobTitle: jobTitle,
            designation: designation, overview: overview,
            areasOfExpertise: areasOfExpertise
        })
    }
    RETURN "Success"

QUERY BatchUpsertAISuitabilities(data: [{
    externalId: String, projectId: String, firmId: String, decision: String,
    confidenceScore: F64, reason: String, citationsPath: String, generatedAt: String,
    vectorDocumentId: String, vectorStoreStatus: String
}]) =>
    FOR { externalId, projectId, firmId, decision, confidenceScore, reason,
          citationsPath, generatedAt, vectorDocumentId, vectorStoreStatus } IN data {
        existing <- N<AISuitability>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, projectId: projectId, firmId: firmId,
            decision: decision, confidenceScore: confidenceScore, reason: reason,
            citationsPath: citationsPath, generatedAt: generatedAt,
            vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertTeamRecommendations(data: [{
    externalId: String, projectId: String, firmId: String, decision: String,
    confidenceScore: F64, reason: String, vectorDocumentId: String,
    vectorStoreStatus: String
}]) =>
    FOR { externalId, projectId, firmId, decision, confidenceScore, reason,
          vectorDocumentId, vectorStoreStatus } IN data {
        existing <- N<TeamRecommendation>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, projectId: projectId, firmId: firmId,
            decision: decision, confidenceScore: confidenceScore, reason: reason,
            vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertEvaluationSections(data: [{
    externalId: String, evaluationId: String, name: String, gcsPath: String,
    projectId: String, firmId: String, vectorDocumentId: String,
    vectorStoreStatus: String
}]) =>
    FOR { externalId, evaluationId, name, gcsPath, projectId, firmId,
          vectorDocumentId, vectorStoreStatus } IN data {
        existing <- N<EvaluationSection>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, evaluationId: evaluationId, name: name,
            gcsPath: gcsPath, projectId: projectId, firmId: firmId,
            vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertInsightNodes(data: [{
    externalId: String, insightsId: String, name: String, gcsPath: String,
    projectId: String, firmId: String, vectorDocumentId: String,
    vectorStoreStatus: String
}]) =>
    FOR { externalId, insightsId, name, gcsPath, projectId, firmId,
          vectorDocumentId, vectorStoreStatus } IN data {
        existing <- N<Insight>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, insightsId: insightsId, name: name,
            gcsPath: gcsPath, projectId: projectId, firmId: firmId,
            vectorDocumentId: vectorDocumentId, vectorStoreStatus: vectorStoreStatus
        })
    }
    RETURN "Success"

QUERY BatchUpsertRfxDocumentsContainers(data: [{externalId: String, projectId: String, firmId: String}]) =>
    FOR { externalId, projectId, firmId } IN data {
        existing <- N<RfxDocuments>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({externalId: externalId, projectId: projectId, firmId: firmId})
    }
    RETURN "Success"

QUERY BatchUpsertContentContainers(data: [{externalId: String, projectId: String, firmId: String}]) =>
    FOR { externalId, projectId, firmId } IN data {
        existing <- N<Content>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({externalId: externalId, projectId: projectId, firmId: firmId})
    }
    RETURN "Success"

QUERY BatchUpsertDocumentChunkTexts(data: [{
    externalId: String, documentId: String, firmId: String, projectId: String,
    sourceType: String, sourceId: String, chunkIndex: I64,
    contentPreview: String,
    summary: String, sectionTitle: String, title: String, headingPath: String
}]) =>
    FOR { externalId, documentId, firmId, projectId, sourceType, sourceId,
          chunkIndex, contentPreview, summary, sectionTitle, title,
          headingPath } IN data {
        existing <- N<DocumentChunkText>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertN({
            externalId: externalId, documentId: documentId, firmId: firmId,
            projectId: projectId, sourceType: sourceType, sourceId: sourceId,
            chunkIndex: chunkIndex, contentPreview: contentPreview,
            summary: summary, sectionTitle: sectionTitle, title: title,
            headingPath: headingPath
        })
    }
    RETURN "Success"

QUERY BatchUpsertDocumentChunks(data: [{
    externalId: String,
    documentId: String, firmId: String, projectId: String,
    sourceType: String, sourceId: String, chunkIndex: I64,
    contentPreview: String, gcsContentPath: String,
    mimeType: String, vectorDocumentId: String, vectorStoreStatus: String,
    summary: String, sectionTitle: String, title: String, headingPath: String,
    pageNumber: I64, proposalId: String, source: String,
    tokenCount: I64, createdAt: String,
    embedding: [F64]
}]) =>
    FOR { externalId, documentId, firmId, projectId, sourceType, sourceId, chunkIndex,
          contentPreview, gcsContentPath, mimeType, vectorDocumentId,
          vectorStoreStatus, summary, sectionTitle, title, headingPath,
          pageNumber, proposalId, source, tokenCount, createdAt,
          embedding } IN data {
        existing <- V<DocumentChunk>::WHERE(_::{externalId}::EQ(externalId))
        result <- existing::UpsertV(embedding, {
            externalId: externalId,
            documentId: documentId, firmId: firmId, projectId: projectId,
            sourceType: sourceType, sourceId: sourceId, chunkIndex: chunkIndex,
            contentPreview: contentPreview, gcsContentPath: gcsContentPath,
            mimeType: mimeType, vectorDocumentId: vectorDocumentId,
            vectorStoreStatus: vectorStoreStatus, summary: summary,
            sectionTitle: sectionTitle, title: title, headingPath: headingPath,
            pageNumber: pageNumber, proposalId: proposalId, source: source,
            tokenCount: tokenCount, createdAt: createdAt
        })
    }
    RETURN "Success"

// Combined upsert: upserts N::DocumentChunkText + V::DocumentChunk + E::ChunkTextHasVector
// for each item. Use this for idempotent ingestion pipeline bulk writes.
QUERY BatchUpsertIngestDocumentChunks(data: [{
    externalId: String, documentId: String, firmId: String, projectId: String,
    sourceType: String, sourceId: String, chunkIndex: I64,
    contentPreview: String, gcsContentPath: String,
    mimeType: String, vectorDocumentId: String, vectorStoreStatus: String,
    summary: String, sectionTitle: String, title: String, headingPath: String,
    pageNumber: I64, proposalId: String, source: String,
    tokenCount: I64, createdAt: String,
    embedding: [F64]
}]) =>
    FOR { externalId, documentId, firmId, projectId, sourceType, sourceId,
          chunkIndex, contentPreview, gcsContentPath, mimeType,
          vectorDocumentId, vectorStoreStatus, summary, sectionTitle,
          title, headingPath, pageNumber, proposalId, source, tokenCount,
          createdAt, embedding } IN data {
        existingText <- N<DocumentChunkText>::WHERE(_::{externalId}::EQ(externalId))
        textNode <- existingText::UpsertN({
            externalId: externalId, documentId: documentId, firmId: firmId,
            projectId: projectId, sourceType: sourceType, sourceId: sourceId,
            chunkIndex: chunkIndex, contentPreview: contentPreview,
            summary: summary, sectionTitle: sectionTitle, title: title,
            headingPath: headingPath
        })
        existingVector <- V<DocumentChunk>::WHERE(_::{externalId}::EQ(externalId))
        vector <- existingVector::UpsertV(embedding, {
            externalId: externalId,
            documentId: documentId, firmId: firmId, projectId: projectId,
            sourceType: sourceType, sourceId: sourceId, chunkIndex: chunkIndex,
            contentPreview: contentPreview, gcsContentPath: gcsContentPath,
            mimeType: mimeType, vectorDocumentId: vectorDocumentId,
            vectorStoreStatus: vectorStoreStatus, summary: summary,
            sectionTitle: sectionTitle, title: title, headingPath: headingPath,
            pageNumber: pageNumber, proposalId: proposalId, source: source,
            tokenCount: tokenCount, createdAt: createdAt
        })
        existingEdge <- E<ChunkTextHasVector>
        edge <- existingEdge::UpsertE({v: "1"})::From(textNode)::To(vector)
    }
    RETURN "Success"

// =============================================================================
// SECTION 8: CDC NODE DELETE
// =============================================================================

QUERY DeleteFirm(externalId: String) =>
    DROP N<Firm>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteUser(externalId: String) =>
    DROP N<User>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteProject(externalId: String) =>
    DROP N<Project>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteVendor(externalId: String) =>
    DROP N<Vendor>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteClient(externalId: String) =>
    DROP N<Client>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteFolder(externalId: String) =>
    DROP N<Folder>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteFile(externalId: String) =>
    DROP N<File>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteProposal(externalId: String) =>
    DROP N<Proposal>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteEvaluation(externalId: String) =>
    DROP N<Evaluation>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteRfxDocument(externalId: String) =>
    DROP N<RfxDocument>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteDocument(externalId: String) =>
    DROP N<Document>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteInsights(externalId: String) =>
    DROP N<Insights>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteInsight(externalId: String) =>
    DROP N<Insight>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteEducation(externalId: String) =>
    DROP N<Education>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteCertification(externalId: String) =>
    DROP N<Certification>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteRegistration(externalId: String) =>
    DROP N<Registration>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteProjectExperience(externalId: String) =>
    DROP N<ProjectExperience>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteClientContact(externalId: String) =>
    DROP N<ClientContact>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteClientLocation(externalId: String) =>
    DROP N<ClientLocation>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteClientKeyRole(externalId: String) =>
    DROP N<ClientKeyRole>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeletePursuitTeamMember(externalId: String) =>
    DROP N<PursuitTeamMember>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeletePursuitTask(externalId: String) =>
    DROP N<PursuitTask>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeletePursuitLocation(externalId: String) =>
    DROP N<PursuitLocation>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteLocation(externalId: String) =>
    DROP N<Location>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteAISuitability(externalId: String) =>
    DROP N<AISuitability>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteTeamRecommendation(externalId: String) =>
    DROP N<TeamRecommendation>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteProjectManager(externalId: String) =>
    DROP N<ProjectManager>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteAreaOfExpertise(externalId: String) =>
    DROP N<AreaOfExpertise>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteEvaluationSection(externalId: String) =>
    DROP N<EvaluationSection>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteDocumentChunkText(externalId: String) =>
    DROP N<DocumentChunkText>::WHERE(_::{externalId}::EQ(externalId))
    RETURN NONE

QUERY DeleteDocumentChunkTextsBySource(sourceType: String, sourceId: String) =>
    DROP N<DocumentChunkText>::WHERE(AND(_::{sourceType}::EQ(sourceType), _::{sourceId}::EQ(sourceId)))
    RETURN NONE

// =============================================================================
// SECTION 9: CDC EDGE UPSERT
// =============================================================================

QUERY UpsertLinkFirmUser(firmId: String, userId: String) =>
    from <- N<Firm>({externalId: firmId})
    to <- N<User>({externalId: userId})
    existing <- E<FirmHasUser>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkFirmProject(firmId: String, projectId: String) =>
    from <- N<Firm>({externalId: firmId})
    to <- N<Project>({externalId: projectId})
    existing <- E<FirmHasProject>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkFirmClient(firmId: String, clientId: String) =>
    from <- N<Firm>({externalId: firmId})
    to <- N<Client>({externalId: clientId})
    existing <- E<FirmHasClient>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkFirmVendor(firmId: String, vendorId: String) =>
    from <- N<Firm>({externalId: firmId})
    to <- N<Vendor>({externalId: vendorId})
    existing <- E<FirmHasVendor>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkFirmFilesRoot(firmId: String, filesRootId: String) =>
    from <- N<Firm>({externalId: firmId})
    to <- N<FilesRoot>({externalId: filesRootId})
    existing <- E<FirmHasFilesRoot>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkFirmProposalTemplate(firmId: String, templateId: String) =>
    from <- N<Firm>({externalId: firmId})
    to <- N<ProposalTemplate>({externalId: templateId})
    existing <- E<FirmHasProposalTemplate>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProjectProposal(projectId: String, proposalId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<Proposal>({externalId: proposalId})
    existing <- E<ProjectHasProposal>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProposalEvaluation(proposalId: String, evaluationId: String) =>
    from <- N<Proposal>({externalId: proposalId})
    to <- N<Evaluation>({externalId: evaluationId})
    existing <- E<ProposalHasEvaluation>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProposalRfxDocuments(proposalId: String, rfxDocsId: String) =>
    from <- N<Proposal>({externalId: proposalId})
    to <- N<RfxDocuments>({externalId: rfxDocsId})
    existing <- E<ProposalHasRfxDocuments>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProposalContent(proposalId: String, contentId: String) =>
    from <- N<Proposal>({externalId: proposalId})
    to <- N<Content>({externalId: contentId})
    existing <- E<ProposalHasContent>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProposalInsights(proposalId: String, insightsId: String) =>
    from <- N<Proposal>({externalId: proposalId})
    to <- N<Insights>({externalId: insightsId})
    existing <- E<ProposalHasInsights>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkEvaluationSection(evaluationId: String, sectionId: String) =>
    from <- N<Evaluation>({externalId: evaluationId})
    to <- N<EvaluationSection>({externalId: sectionId})
    existing <- E<EvaluationHasSection>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkInsightsInsight(insightsId: String, insightId: String) =>
    from <- N<Insights>({externalId: insightsId})
    to <- N<Insight>({externalId: insightId})
    existing <- E<InsightsHasInsight>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkUserExpertise(userId: String, expertiseId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<AreaOfExpertise>({externalId: expertiseId})
    existing <- E<UserHasExpertise>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkUserRegistration(userId: String, registrationId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<Registration>({externalId: registrationId})
    existing <- E<UserHasRegistration>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkClientContact(clientId: String, contactId: String) =>
    from <- N<Client>({externalId: clientId})
    to <- N<ClientContact>({externalId: contactId})
    existing <- E<ClientHasContact>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkClientKeyRole(clientId: String, keyRoleId: String) =>
    from <- N<Client>({externalId: clientId})
    to <- N<ClientKeyRole>({externalId: keyRoleId})
    existing <- E<ClientHasKeyRole>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkFilesRootFolder(filesRootId: String, folderId: String) =>
    from <- N<FilesRoot>({externalId: filesRootId})
    to <- N<Folder>({externalId: folderId})
    existing <- E<FilesRootContainsFolder>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkFilesRootFile(filesRootId: String, fileId: String) =>
    from <- N<FilesRoot>({externalId: filesRootId})
    to <- N<File>({externalId: fileId})
    existing <- E<FilesRootContainsFile>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkFolderFolder(parentFolderId: String, childFolderId: String) =>
    from <- N<Folder>({externalId: parentFolderId})
    to <- N<Folder>({externalId: childFolderId})
    existing <- E<FolderContainsFolder>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkFolderFile(folderId: String, fileId: String) =>
    from <- N<Folder>({externalId: folderId})
    to <- N<File>({externalId: fileId})
    existing <- E<FolderContainsFile>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkContentDocument(contentId: String, documentId: String) =>
    from <- N<Content>({externalId: contentId})
    to <- N<Document>({externalId: documentId})
    existing <- E<ContentContainsDocument>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkRfxDocsRfxDoc(rfxDocsId: String, rfxDocId: String) =>
    from <- N<RfxDocuments>({externalId: rfxDocsId})
    to <- N<RfxDocument>({externalId: rfxDocId})
    existing <- E<RfxDocsContainsRfxDoc>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProjectClient(projectId: String, clientId: String, role: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<Client>({externalId: clientId})
    DROP from::OutE<HasClient>::WHERE(_::{role}::EQ(role))
    edge <- AddE<HasClient>({role: role, v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkClientProject(clientId: String, projectId: String, role: String) =>
    from <- N<Client>({externalId: clientId})
    to <- N<Project>({externalId: projectId})
    DROP from::OutE<EngagedOn>::WHERE(AND(_::{role}::EQ(role), _::{targetExternalId}::EQ(projectId)))
    edge <- AddE<EngagedOn>({role: role, v: "1", targetExternalId: projectId})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProjectManager(projectId: String, pmId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<ProjectManager>({externalId: pmId})
    existing <- E<LedBy>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProjectSuitability(projectId: String, suitabilityId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<AISuitability>({externalId: suitabilityId})
    existing <- E<HasSuitability>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProjectRecommendation(projectId: String, recommendationId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<TeamRecommendation>({externalId: recommendationId})
    existing <- E<HasRecommendation>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProjectLocation(projectId: String, locationId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<Location>({externalId: locationId})
    existing <- E<ConductedAt>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProjectFile(projectId: String, fileId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<File>({externalId: fileId})
    existing <- E<HasFile>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkUserEducation(userId: String, educationId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<Education>({externalId: educationId})
    existing <- E<Attended>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkUserCertification(userId: String, certificationId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<Certification>({externalId: certificationId})
    existing <- E<Completed>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkUserProjectExperience(userId: String, experienceId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<ProjectExperience>({externalId: experienceId})
    existing <- E<WorkedOn>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkUserIsProjectManager(userId: String, pmId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<ProjectManager>({externalId: pmId})
    existing <- E<IsA>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkClientChild(childClientId: String, parentClientId: String) =>
    from <- N<Client>({externalId: childClientId})
    to <- N<Client>({externalId: parentClientId})
    existing <- E<ChildOf>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkClientLocation(clientId: String, locationId: String) =>
    from <- N<Client>({externalId: clientId})
    to <- N<ClientLocation>({externalId: locationId})
    existing <- E<LocatedAt>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkKeyRoleContact(keyRoleId: String, contactId: String) =>
    from <- N<ClientKeyRole>({externalId: keyRoleId})
    to <- N<ClientContact>({externalId: contactId})
    existing <- E<FilledBy>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProjectTeamMember(projectId: String, teamMemberId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<PursuitTeamMember>({externalId: teamMemberId})
    existing <- E<HasTeamMember>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkTeamMemberUser(teamMemberId: String, userId: String) =>
    from <- N<PursuitTeamMember>({externalId: teamMemberId})
    to <- N<User>({externalId: userId})
    existing <- E<IsUser>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkUserToTeamMember(userId: String, teamMemberId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<PursuitTeamMember>({externalId: teamMemberId})
    existing <- E<UserAssignedToTeamMember>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkContactToProject(contactId: String, projectId: String, role: String, targetExternalId: String) =>
    from <- N<ClientContact>({externalId: contactId})
    to <- N<Project>({externalId: projectId})
    DROP from::OutE<ContactAssignedToProject>::WHERE(_::{targetExternalId}::EQ(targetExternalId))
    edge <- AddE<ContactAssignedToProject>({role: role, v: "1", targetExternalId: targetExternalId})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkTeamMemberTask(teamMemberId: String, taskId: String) =>
    from <- N<PursuitTeamMember>({externalId: teamMemberId})
    to <- N<PursuitTask>({externalId: taskId})
    existing <- E<HasTask>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProjectPursuitLocation(projectId: String, pursuitLocationId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<PursuitLocation>({externalId: pursuitLocationId})
    existing <- E<HasPursuitLocation>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProjectClientContact(projectId: String, contactId: String, role: String, targetExternalId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<ClientContact>({externalId: contactId})
    DROP from::OutE<ProjectHasClientContact>::WHERE(_::{targetExternalId}::EQ(targetExternalId))
    edge <- AddE<ProjectHasClientContact>({role: role, v: "1", targetExternalId: targetExternalId})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkProjectDerivedTemplate(projectId: String, templateId: String) =>
    from <- N<Project>({externalId: projectId})
    to <- N<ProposalTemplate>({externalId: templateId})
    existing <- E<DerivedTemplate>
    edge <- existing::UpsertE({v: "1"})::From(from)::To(to)
    RETURN edge

QUERY UpsertLinkWorkerProjectAssignment(userId: String, projectId: String) =>
    from <- N<User>({externalId: userId})
    to <- N<Project>({externalId: projectId})
    existing <- E<WorksOnProject>
    edge <- existing::UpsertE({v: "1", targetExternalId: ""})::From(from)::To(to)
    RETURN edge

// NOTE: Edge deletion IS supported in HelixDB via DROP node::OutE<Type>.
// Targeted edge deletion queries (DropUserExpertiseEdges, etc.) are defined
// in Section 11 alongside other bulk-operation DROP queries.
// =============================================================================
// SECTION 10: CDC VECTOR OPERATIONS
// =============================================================================

QUERY AddDocumentChunkWithEmbed(
    documentId: String, firmId: String, projectId: String,
    sourceType: String, sourceId: String, chunkIndex: I64,
    contentPreview: String, gcsContentPath: String,
    mimeType: String, vectorDocumentId: String, vectorStoreStatus: String,
    summary: String, sectionTitle: String, title: String, headingPath: String,
    pageNumber: I64, proposalId: String, source: String,
    tokenCount: I64, createdAt: String,
    text: String
) =>
    result <- AddV<DocumentChunk>(Embed(text), {
        documentId: documentId, firmId: firmId, projectId: projectId,
        sourceType: sourceType, sourceId: sourceId, chunkIndex: chunkIndex,
        contentPreview: contentPreview, gcsContentPath: gcsContentPath,
        mimeType: mimeType, vectorDocumentId: vectorDocumentId,
        vectorStoreStatus: vectorStoreStatus, summary: summary,
        sectionTitle: sectionTitle, title: title, headingPath: headingPath,
        pageNumber: pageNumber, proposalId: proposalId, source: source,
        tokenCount: tokenCount, createdAt: createdAt
    })
    RETURN result

QUERY DeleteDocumentChunksBySource(sourceType: String, sourceId: String) =>
    DROP V<DocumentChunk>::WHERE(AND(_::{sourceType}::EQ(sourceType), _::{sourceId}::EQ(sourceId)))
    RETURN NONE

QUERY DeleteDocumentChunksByDocumentId(documentId: String) =>
    DROP V<DocumentChunk>::WHERE(_::{documentId}::EQ(documentId))
    RETURN NONE

// --- Document chunk lookup queries (for migration idempotency + management) ---

// Look up a chunk text node by externalId (idempotency check)
QUERY GetDocumentChunkTextByExternalId(externalId: String) =>
    result <- N<DocumentChunkText>({externalId: externalId})::!{id, label}
    RETURN result

// Look up all chunk texts for a document (for deletion pipeline)
QUERY GetDocumentChunkTextsByDocumentId(documentId: String) =>
    results <- N<DocumentChunkText>::WHERE(_::{documentId}::EQ(documentId))::!{id, label}
    RETURN results

// Look up chunk texts by source (for dedup checking)
QUERY GetDocumentChunkTextsBySource(sourceType: String, sourceId: String) =>
    results <- N<DocumentChunkText>::WHERE(AND(_::{sourceType}::EQ(sourceType), _::{sourceId}::EQ(sourceId)))::!{id, label}
    RETURN results

// Look up all chunk texts for a firm (for migration verification)
QUERY GetDocumentChunkTextsByFirm(firmId: String, start: U32, end: U32) =>
    results <- N<DocumentChunkText>::WHERE(_::{firmId}::EQ(firmId))::RANGE(start, end)::!{id, label}
    RETURN results

// Delete ALL chunks (both N:: and V::) for a document - full cleanup
QUERY DeleteAllChunksByDocumentId(documentId: String) =>
    DROP N<DocumentChunkText>::WHERE(_::{documentId}::EQ(documentId))
    DROP V<DocumentChunk>::WHERE(_::{documentId}::EQ(documentId))
    RETURN NONE

// Delete ALL chunks (both N:: and V::) for a source entity - full cleanup
QUERY DeleteAllChunksBySource(sourceType: String, sourceId: String) =>
    DROP N<DocumentChunkText>::WHERE(AND(_::{sourceType}::EQ(sourceType), _::{sourceId}::EQ(sourceId)))
    DROP V<DocumentChunk>::WHERE(AND(_::{sourceType}::EQ(sourceType), _::{sourceId}::EQ(sourceId)))
    RETURN NONE

// Count chunk texts for a firm (migration progress tracking)
// NOTE: Not #[mcp] because ::COUNT returns scalar, incompatible with MCP codegen
QUERY CountDocumentChunkTextsByFirm(firmId: String) =>
    results <- N<DocumentChunkText>::WHERE(_::{firmId}::EQ(firmId))::COUNT
    RETURN results

// =============================================================================
// SECTION 11: BATCH OPERATIONS
//
// HelixDB supports batched queries via FOR loops over array parameters.
// Use these for bulk ingestion and seed operations to reduce round-trips.
//
// Pattern:
//   QUERY BatchAdd<Type>(data: [{field1: Type1, field2: Type2, ...}]) =>
//       FOR { field1, field2, ... } IN data {
//           result <- AddN<Type>({field1: field1, field2: field2, ...})
//       }
//       RETURN "Success"
//
// The same pattern works for edges:
//   QUERY BatchLink<Edge>(data: [{fromId: String, toId: String}]) =>
//       FOR { fromId, toId } IN data {
//           from <- N<FromType>({externalId: fromId})
//           to <- N<ToType>({externalId: toId})
//           result <- AddE<EdgeType>::From(from)::To(to)
//       }
//       RETURN "Success"
// =============================================================================

// --- Batch document ingestion (critical path — replaces TurboPuffer bulk writes) ---

// Combined batch: creates N::DocumentChunkText + V::DocumentChunk + E::ChunkTextHasVector
// for each item. Use this for ingestion pipeline bulk writes.
QUERY BatchIngestDocumentChunks(data: [{
    externalId: String, documentId: String, firmId: String, projectId: String,
    sourceType: String, sourceId: String, chunkIndex: I64,
    contentPreview: String, gcsContentPath: String,
    mimeType: String, vectorDocumentId: String, vectorStoreStatus: String,
    summary: String, sectionTitle: String, title: String, headingPath: String,
    pageNumber: I64, proposalId: String, source: String,
    tokenCount: I64, createdAt: String,
    embedding: [F64]
}]) =>
    FOR { externalId, documentId, firmId, projectId, sourceType, sourceId,
          chunkIndex, contentPreview, gcsContentPath, mimeType,
          vectorDocumentId, vectorStoreStatus, summary, sectionTitle,
          title, headingPath, pageNumber, proposalId, source, tokenCount,
          createdAt, embedding } IN data {
        textNode <- AddN<DocumentChunkText>({
            externalId: externalId, documentId: documentId, firmId: firmId,
            projectId: projectId, sourceType: sourceType, sourceId: sourceId,
            chunkIndex: chunkIndex, contentPreview: contentPreview,
            summary: summary, sectionTitle: sectionTitle, title: title,
            headingPath: headingPath
        })
        vector <- AddV<DocumentChunk>(embedding, {
            documentId: documentId, firmId: firmId, projectId: projectId,
            sourceType: sourceType, sourceId: sourceId, chunkIndex: chunkIndex,
            contentPreview: contentPreview, gcsContentPath: gcsContentPath,
            mimeType: mimeType, vectorDocumentId: vectorDocumentId,
            vectorStoreStatus: vectorStoreStatus, summary: summary,
            sectionTitle: sectionTitle, title: title, headingPath: headingPath,
            pageNumber: pageNumber, proposalId: proposalId, source: source,
            tokenCount: tokenCount, createdAt: createdAt
        })
        edge <- AddE<ChunkTextHasVector>({v: "1"})::From(textNode)::To(vector)
    }
    RETURN "Success"

// --- Batch seed: Idempotent edge upserts (UpsertE) ---

QUERY BatchUpsertLinkFirmUsers(data: [{firmId: String, userId: String}]) =>
    FOR { firmId, userId } IN data {
        from <- N<Firm>({externalId: firmId})
        to <- N<User>({externalId: userId})
        existing <- E<FirmHasUser>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkFirmProjects(data: [{firmId: String, projectId: String}]) =>
    FOR { firmId, projectId } IN data {
        from <- N<Firm>({externalId: firmId})
        to <- N<Project>({externalId: projectId})
        existing <- E<FirmHasProject>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkFirmClients(data: [{firmId: String, clientId: String}]) =>
    FOR { firmId, clientId } IN data {
        from <- N<Firm>({externalId: firmId})
        to <- N<Client>({externalId: clientId})
        existing <- E<FirmHasClient>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkFolderFiles(data: [{folderId: String, fileId: String}]) =>
    FOR { folderId, fileId } IN data {
        from <- N<Folder>({externalId: folderId})
        to <- N<File>({externalId: fileId})
        existing <- E<FolderContainsFile>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkFolderFolders(data: [{parentFolderId: String, childFolderId: String}]) =>
    FOR { parentFolderId, childFolderId } IN data {
        from <- N<Folder>({externalId: parentFolderId})
        to <- N<Folder>({externalId: childFolderId})
        existing <- E<FolderContainsFolder>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProjectFiles(data: [{projectId: String, fileId: String}]) =>
    FOR { projectId, fileId } IN data {
        from <- N<Project>({externalId: projectId})
        to <- N<File>({externalId: fileId})
        existing <- E<HasFile>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkUserEducations(data: [{userId: String, educationId: String}]) =>
    FOR { userId, educationId } IN data {
        from <- N<User>({externalId: userId})
        to <- N<Education>({externalId: educationId})
        existing <- E<Attended>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkUserCertifications(data: [{userId: String, certificationId: String}]) =>
    FOR { userId, certificationId } IN data {
        from <- N<User>({externalId: userId})
        to <- N<Certification>({externalId: certificationId})
        existing <- E<Completed>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkUserProjectExperiences(data: [{userId: String, experienceId: String}]) =>
    FOR { userId, experienceId } IN data {
        from <- N<User>({externalId: userId})
        to <- N<ProjectExperience>({externalId: experienceId})
        existing <- E<WorkedOn>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkUserExpertises(data: [{userId: String, expertiseId: String}]) =>
    FOR { userId, expertiseId } IN data {
        from <- N<User>({externalId: userId})
        to <- N<AreaOfExpertise>({externalId: expertiseId})
        existing <- E<UserHasExpertise>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkFirmVendors(data: [{firmId: String, vendorId: String}]) =>
    FOR { firmId, vendorId } IN data {
        from <- N<Firm>({externalId: firmId})
        to <- N<Vendor>({externalId: vendorId})
        existing <- E<FirmHasVendor>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkFirmFilesRoots(data: [{firmId: String, filesRootId: String}]) =>
    FOR { firmId, filesRootId } IN data {
        from <- N<Firm>({externalId: firmId})
        to <- N<FilesRoot>({externalId: filesRootId})
        existing <- E<FirmHasFilesRoot>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProjectClients(data: [{projectId: String, clientId: String, role: String}]) =>
    FOR { projectId, clientId, role } IN data {
        from <- N<Project>({externalId: projectId})
        to <- N<Client>({externalId: clientId})
        existing <- E<HasClient>
        result <- existing::UpsertE({role: role, v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkClientProjects(data: [{clientId: String, projectId: String, role: String}]) =>
    FOR { clientId, projectId, role } IN data {
        from <- N<Client>({externalId: clientId})
        to <- N<Project>({externalId: projectId})
        existing <- E<EngagedOn>
        result <- existing::UpsertE({role: role, v: "1", targetExternalId: projectId})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProjectManagers(data: [{projectId: String, pmId: String}]) =>
    FOR { projectId, pmId } IN data {
        from <- N<Project>({externalId: projectId})
        to <- N<ProjectManager>({externalId: pmId})
        existing <- E<LedBy>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProjectLocations(data: [{projectId: String, locationId: String}]) =>
    FOR { projectId, locationId } IN data {
        from <- N<Project>({externalId: projectId})
        to <- N<Location>({externalId: locationId})
        existing <- E<ConductedAt>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProjectSuitabilities(data: [{projectId: String, suitabilityId: String}]) =>
    FOR { projectId, suitabilityId } IN data {
        from <- N<Project>({externalId: projectId})
        to <- N<AISuitability>({externalId: suitabilityId})
        existing <- E<HasSuitability>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProjectRecommendations(data: [{projectId: String, recommendationId: String}]) =>
    FOR { projectId, recommendationId } IN data {
        from <- N<Project>({externalId: projectId})
        to <- N<TeamRecommendation>({externalId: recommendationId})
        existing <- E<HasRecommendation>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProjectProposals(data: [{projectId: String, proposalId: String}]) =>
    FOR { projectId, proposalId } IN data {
        from <- N<Project>({externalId: projectId})
        to <- N<Proposal>({externalId: proposalId})
        existing <- E<ProjectHasProposal>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProposalRfxDocuments(data: [{proposalId: String, rfxDocsId: String}]) =>
    FOR { proposalId, rfxDocsId } IN data {
        from <- N<Proposal>({externalId: proposalId})
        to <- N<RfxDocuments>({externalId: rfxDocsId})
        existing <- E<ProposalHasRfxDocuments>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProposalContents(data: [{proposalId: String, contentId: String}]) =>
    FOR { proposalId, contentId } IN data {
        from <- N<Proposal>({externalId: proposalId})
        to <- N<Content>({externalId: contentId})
        existing <- E<ProposalHasContent>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProposalEvaluations(data: [{proposalId: String, evaluationId: String}]) =>
    FOR { proposalId, evaluationId } IN data {
        from <- N<Proposal>({externalId: proposalId})
        to <- N<Evaluation>({externalId: evaluationId})
        existing <- E<ProposalHasEvaluation>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkEvaluationSections(data: [{evaluationId: String, sectionId: String}]) =>
    FOR { evaluationId, sectionId } IN data {
        from <- N<Evaluation>({externalId: evaluationId})
        to <- N<EvaluationSection>({externalId: sectionId})
        existing <- E<EvaluationHasSection>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProposalInsights(data: [{proposalId: String, insightsId: String}]) =>
    FOR { proposalId, insightsId } IN data {
        from <- N<Proposal>({externalId: proposalId})
        to <- N<Insights>({externalId: insightsId})
        existing <- E<ProposalHasInsights>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkInsightsInsights(data: [{insightsId: String, insightId: String}]) =>
    FOR { insightsId, insightId } IN data {
        from <- N<Insights>({externalId: insightsId})
        to <- N<Insight>({externalId: insightId})
        existing <- E<InsightsHasInsight>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkRfxDocsRfxDocs(data: [{rfxDocsId: String, rfxDocId: String}]) =>
    FOR { rfxDocsId, rfxDocId } IN data {
        from <- N<RfxDocuments>({externalId: rfxDocsId})
        to <- N<RfxDocument>({externalId: rfxDocId})
        existing <- E<RfxDocsContainsRfxDoc>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkContentDocuments(data: [{contentId: String, documentId: String}]) =>
    FOR { contentId, documentId } IN data {
        from <- N<Content>({externalId: contentId})
        to <- N<Document>({externalId: documentId})
        existing <- E<ContentContainsDocument>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkUserRegistrations(data: [{userId: String, registrationId: String}]) =>
    FOR { userId, registrationId } IN data {
        from <- N<User>({externalId: userId})
        to <- N<Registration>({externalId: registrationId})
        existing <- E<UserHasRegistration>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkUserIsProjectManagers(data: [{userId: String, pmId: String}]) =>
    FOR { userId, pmId } IN data {
        from <- N<User>({externalId: userId})
        to <- N<ProjectManager>({externalId: pmId})
        existing <- E<IsA>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkClientContacts(data: [{clientId: String, contactId: String}]) =>
    FOR { clientId, contactId } IN data {
        from <- N<Client>({externalId: clientId})
        to <- N<ClientContact>({externalId: contactId})
        existing <- E<ClientHasContact>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkClientLocations(data: [{clientId: String, locationId: String}]) =>
    FOR { clientId, locationId } IN data {
        from <- N<Client>({externalId: clientId})
        to <- N<ClientLocation>({externalId: locationId})
        existing <- E<LocatedAt>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkClientKeyRoles(data: [{clientId: String, keyRoleId: String}]) =>
    FOR { clientId, keyRoleId } IN data {
        from <- N<Client>({externalId: clientId})
        to <- N<ClientKeyRole>({externalId: keyRoleId})
        existing <- E<ClientHasKeyRole>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkKeyRoleContacts(data: [{keyRoleId: String, contactId: String}]) =>
    FOR { keyRoleId, contactId } IN data {
        from <- N<ClientKeyRole>({externalId: keyRoleId})
        to <- N<ClientContact>({externalId: contactId})
        existing <- E<FilledBy>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkClientChildren(data: [{childClientId: String, parentClientId: String}]) =>
    FOR { childClientId, parentClientId } IN data {
        from <- N<Client>({externalId: childClientId})
        to <- N<Client>({externalId: parentClientId})
        existing <- E<ChildOf>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkFilesRootFolders(data: [{filesRootId: String, folderId: String}]) =>
    FOR { filesRootId, folderId } IN data {
        from <- N<FilesRoot>({externalId: filesRootId})
        to <- N<Folder>({externalId: folderId})
        existing <- E<FilesRootContainsFolder>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkFilesRootFiles(data: [{filesRootId: String, fileId: String}]) =>
    FOR { filesRootId, fileId } IN data {
        from <- N<FilesRoot>({externalId: filesRootId})
        to <- N<File>({externalId: fileId})
        existing <- E<FilesRootContainsFile>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProjectTeamMembers(data: [{projectId: String, teamMemberId: String}]) =>
    FOR { projectId, teamMemberId } IN data {
        from <- N<Project>({externalId: projectId})
        to <- N<PursuitTeamMember>({externalId: teamMemberId})
        existing <- E<HasTeamMember>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkTeamMemberUsers(data: [{teamMemberId: String, userId: String}]) =>
    FOR { teamMemberId, userId } IN data {
        from <- N<PursuitTeamMember>({externalId: teamMemberId})
        to <- N<User>({externalId: userId})
        existing <- E<IsUser>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkUserToTeamMembers(data: [{userId: String, teamMemberId: String}]) =>
    FOR { userId, teamMemberId } IN data {
        from <- N<User>({externalId: userId})
        to <- N<PursuitTeamMember>({externalId: teamMemberId})
        existing <- E<UserAssignedToTeamMember>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkTeamMemberTasks(data: [{teamMemberId: String, taskId: String}]) =>
    FOR { teamMemberId, taskId } IN data {
        from <- N<PursuitTeamMember>({externalId: teamMemberId})
        to <- N<PursuitTask>({externalId: taskId})
        existing <- E<HasTask>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProjectPursuitLocations(data: [{projectId: String, pursuitLocationId: String}]) =>
    FOR { projectId, pursuitLocationId } IN data {
        from <- N<Project>({externalId: projectId})
        to <- N<PursuitLocation>({externalId: pursuitLocationId})
        existing <- E<HasPursuitLocation>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkFirmProposalTemplates(data: [{firmId: String, templateId: String}]) =>
    FOR { firmId, templateId } IN data {
        from <- N<Firm>({externalId: firmId})
        to <- N<ProposalTemplate>({externalId: templateId})
        existing <- E<FirmHasProposalTemplate>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProjectDerivedTemplates(data: [{projectId: String, templateId: String}]) =>
    FOR { projectId, templateId } IN data {
        from <- N<Project>({externalId: projectId})
        to <- N<ProposalTemplate>({externalId: templateId})
        existing <- E<DerivedTemplate>
        result <- existing::UpsertE({v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkContactToProjects(data: [{contactId: String, projectId: String, role: String}]) =>
    FOR { contactId, projectId, role } IN data {
        from <- N<ClientContact>({externalId: contactId})
        to <- N<Project>({externalId: projectId})
        existing <- E<ContactAssignedToProject>
        result <- existing::UpsertE({role: role, v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY BatchUpsertLinkProjectClientContacts(data: [{projectId: String, contactId: String, role: String}]) =>
    FOR { projectId, contactId, role } IN data {
        from <- N<Project>({externalId: projectId})
        to <- N<ClientContact>({externalId: contactId})
        existing <- E<ProjectHasClientContact>
        result <- existing::UpsertE({role: role, v: "1"})::From(from)::To(to)
    }
    RETURN "Success"

QUERY DropFirmEdges(firmId: String) =>
    node <- N<Firm>({externalId: firmId})
    DROP node::OutE<FirmHasUser>
    DROP node::OutE<FirmHasProject>
    DROP node::OutE<FirmHasClient>
    DROP node::OutE<FirmHasVendor>
    DROP node::OutE<FirmHasFilesRoot>
    DROP node::OutE<FirmHasProposalTemplate>
    RETURN "Success"

QUERY DropProjectEdges(projectId: String) =>
    node <- N<Project>({externalId: projectId})
    DROP node::OutE<ProjectHasProposal>
    DROP node::OutE<HasTeamMember>
    DROP node::OutE<HasClient>
    DROP node::OutE<LedBy>
    DROP node::OutE<HasSuitability>
    DROP node::OutE<HasRecommendation>
    DROP node::OutE<ConductedAt>
    DROP node::OutE<HasFile>
    DROP node::OutE<HasPursuitLocation>
    DROP node::OutE<ProjectHasClientContact>
    DROP node::OutE<DerivedTemplate>
    RETURN "Success"

QUERY DropUserEdges(userId: String) =>
    node <- N<User>({externalId: userId})
    DROP node::OutE<Attended>
    DROP node::OutE<Completed>
    DROP node::OutE<WorkedOn>
    DROP node::OutE<IsA>
    DROP node::OutE<UserHasExpertise>
    DROP node::OutE<UserHasRegistration>
    DROP node::OutE<UserAssignedToTeamMember>
    DROP node::OutE<WorksOnProject>
    RETURN "Success"

QUERY DropClientEdges(clientId: String) =>
    node <- N<Client>({externalId: clientId})
    DROP node::OutE<ClientHasContact>
    DROP node::OutE<ClientHasKeyRole>
    DROP node::OutE<ChildOf>
    DROP node::OutE<LocatedAt>
    DROP node::OutE<EngagedOn>
    RETURN "Success"

QUERY DropProposalEdges(proposalId: String) =>
    node <- N<Proposal>({externalId: proposalId})
    DROP node::OutE<ProposalHasEvaluation>
    DROP node::OutE<ProposalHasRfxDocuments>
    DROP node::OutE<ProposalHasContent>
    DROP node::OutE<ProposalHasInsights>
    RETURN "Success"

QUERY DropEvaluationEdges(evaluationId: String) =>
    node <- N<Evaluation>({externalId: evaluationId})
    DROP node::OutE<EvaluationHasSection>
    RETURN "Success"

QUERY DropInsightsEdges(insightsId: String) =>
    node <- N<Insights>({externalId: insightsId})
    DROP node::OutE<InsightsHasInsight>
    RETURN "Success"

QUERY DropRfxDocsEdges(rfxDocsId: String) =>
    node <- N<RfxDocuments>({externalId: rfxDocsId})
    DROP node::OutE<RfxDocsContainsRfxDoc>
    RETURN "Success"

QUERY DropContentEdges(contentId: String) =>
    node <- N<Content>({externalId: contentId})
    DROP node::OutE<ContentContainsDocument>
    RETURN "Success"

QUERY DropFilesRootEdges(filesRootId: String) =>
    node <- N<FilesRoot>({externalId: filesRootId})
    DROP node::OutE<FilesRootContainsFolder>
    DROP node::OutE<FilesRootContainsFile>
    RETURN "Success"

QUERY DropFolderEdges(folderId: String) =>
    node <- N<Folder>({externalId: folderId})
    DROP node::OutE<FolderContainsFolder>
    DROP node::OutE<FolderContainsFile>
    RETURN "Success"

QUERY DropTeamMemberEdges(teamMemberId: String) =>
    node <- N<PursuitTeamMember>({externalId: teamMemberId})
    DROP node::OutE<IsUser>
    DROP node::OutE<HasTask>
    RETURN "Success"

QUERY DropKeyRoleEdges(keyRoleId: String) =>
    node <- N<ClientKeyRole>({externalId: keyRoleId})
    DROP node::OutE<FilledBy>
    RETURN "Success"

QUERY DropContactEdges(contactId: String) =>
    node <- N<ClientContact>({externalId: contactId})
    DROP node::OutE<ContactAssignedToProject>
    RETURN "Success"

// --- Targeted DROP queries for special updates (partial edge deletion) ---

QUERY DropUserExpertiseEdges(userId: String) =>
    node <- N<User>({externalId: userId})
    DROP node::OutE<UserHasExpertise>
    RETURN "Success"

QUERY DropClientChildEdge(clientId: String) =>
    node <- N<Client>({externalId: clientId})
    DROP node::OutE<ChildOf>
    RETURN "Success"

QUERY DropKeyRoleFilledByEdge(keyRoleId: String) =>
    node <- N<ClientKeyRole>({externalId: keyRoleId})
    DROP node::OutE<FilledBy>
    RETURN "Success"

QUERY DropProjectLedByEdge(projectId: String) =>
    node <- N<Project>({externalId: projectId})
    DROP node::OutE<LedBy>
    RETURN "Success"

QUERY DropProjectClientEdgesByRole(projectId: String, role: String) =>
    node <- N<Project>({externalId: projectId})
    DROP node::OutE<HasClient>::WHERE(_::{role}::EQ(role))
    RETURN "Success"

QUERY DropClientEngagedOnByProject(clientId: String, projectId: String) =>
    node <- N<Client>({externalId: clientId})
    DROP node::OutE<EngagedOn>::WHERE(_::{targetExternalId}::EQ(projectId))
    RETURN "Success"

QUERY DropWorkerProjectAssignment(userId: String, projectId: String) =>
    node <- N<User>({externalId: userId})
    DROP node::OutE<WorksOnProject>::WHERE(_::{targetExternalId}::EQ(projectId))
    RETURN "Success"

QUERY DropContactToProjectEdge(contactId: String, targetExternalId: String) =>
    node <- N<ClientContact>({externalId: contactId})
    DROP node::OutE<ContactAssignedToProject>::WHERE(_::{targetExternalId}::EQ(targetExternalId))
    RETURN "Success"

QUERY DropProjectClientContactEdge(projectId: String, targetExternalId: String) =>
    node <- N<Project>({externalId: projectId})
    DROP node::OutE<ProjectHasClientContact>::WHERE(_::{targetExternalId}::EQ(targetExternalId))
    RETURN "Success"

// --- Task lookup for read-modify-write (comment aggregation) ---

QUERY GetPursuitTaskByExternalId(externalId: String) =>
    result <- N<PursuitTask>({externalId: externalId})::!{id, label}
    RETURN result
