QUERY CreateFullAppWithPages (user_id: ID, app_name: String, app_description: String, created_at: Date) =>
    // Get the user record
    user <- N<User>(user_id)
    
    // Create the main app
    app <- AddN<App>({
        name: app_name,
        description: app_description,
        created_at: created_at,
    })
    
    // Create branches
    dev_branch <- AddN<Branch>({
        name: "Development"
    })
    
    staging_branch <- AddN<Branch>({
        name: "Staging"
    })
    
    // Create frontend and backend instances for dev branch
    frontend_dev <- AddN<Frontend>
    backend_dev <- AddN<Backend>
    
    // Create frontend and backend instances for staging branch
    frontend_staging <- AddN<Frontend>
    backend_staging <- AddN<Backend>
    
    // Create elements for pages
    root_element <- AddN<Element>({
        element_id: "root_element",
        name: "root_element"
    })
    
    root_element_404 <- AddN<Element>({
        element_id: "root_element",
        name: "root_element"
    })
    
    root_element_reset <- AddN<Element>({
        element_id: "root_element", 
        name: "root_element"
    })
    
    // Create pages
    index_page <- AddN<Page>({
        name: "index"
    })
    
    not_found_page <- AddN<Page>({
        name: "Page not found"
    })
    
    reset_password_page <- AddN<Page>({
        name: "Reset Password"
    })
    
    // Create main folder
    main_folder <- AddN<PageFolder>({
        name: "Unsorted"
    })
    
    // Create relationships - User to App
    user_app_edge <- AddE<User_Has_Access_To>({
        assigned_at: created_at
    })::From(user)::To(app)
    
    // App to Branches
    app_dev_branch_edge <- AddE<App_Has_Branch>({
        created_at: created_at
    })::From(app)::To(dev_branch)
    
    app_staging_branch_edge <- AddE<App_Has_Branch>({
        created_at: created_at
    })::From(app)::To(staging_branch)
    
    // Branch to Frontend/Backend - Development
    dev_branch_frontend_edge <- AddE<Branch_Has_Frontend>({
        created_at: created_at
    })::From(dev_branch)::To(frontend_dev)
    
    dev_branch_backend_edge <- AddE<Branch_Has_Backend>({
        created_at: created_at
    })::From(dev_branch)::To(backend_dev)
    
    // Branch to Frontend/Backend - Staging
    staging_branch_frontend_edge <- AddE<Branch_Has_Frontend>({
        created_at: created_at
    })::From(staging_branch)::To(frontend_staging)
    
    staging_branch_backend_edge <- AddE<Branch_Has_Backend>({
        created_at: created_at
    })::From(staging_branch)::To(backend_staging)
    
    // Page to Element relationships
    index_page_element_edge <- AddE<Page_Has_Root_Element>({
        assigned_at: created_at
    })::From(index_page)::To(root_element)
    
    not_found_page_element_edge <- AddE<Page_Has_Root_Element>({
        assigned_at: created_at
    })::From(not_found_page)::To(root_element_404)
    
    reset_page_element_edge <- AddE<Page_Has_Root_Element>({
        assigned_at: created_at
    })::From(reset_password_page)::To(root_element_reset)
    
    // PageFolder to Page relationships
    folder_index_edge <- AddE<PageFolder_Contains_Page>({
        assigned_at: created_at
    })::From(main_folder)::To(index_page)
    
    folder_404_edge <- AddE<PageFolder_Contains_Page>({
        assigned_at: created_at
    })::From(main_folder)::To(not_found_page)
    
    folder_reset_edge <- AddE<PageFolder_Contains_Page>({
        assigned_at: created_at
    })::From(main_folder)::To(reset_password_page)
    
    // Frontend to Page relationships (for dev)
    frontend_index_edge <- AddE<Frontend_Has_Page>({
        assigned_at: created_at
    })::From(frontend_dev)::To(index_page)
    
    frontend_404_edge <- AddE<Frontend_Has_Page>({
        assigned_at: created_at
    })::From(frontend_dev)::To(not_found_page)
    
    frontend_reset_edge <- AddE<Frontend_Has_Page>({
        assigned_at: created_at
    })::From(frontend_dev)::To(reset_password_page)
    
    // Frontend to PageFolder relationship
    frontend_folder_edge <- AddE<Frontend_Contains_PageFolder>({
        assigned_at: created_at
    })::From(frontend_dev)::To(main_folder)
    
    // Return the created app with nested structure
    // RETURN app::{
    //     branches: {
    //         dev_branch: dev_branch::|dev|{
    //             name: dev::{name},
    //             frontend: frontend_dev::{
    //                 pages: {index_page, not_found_page, reset_password_page},
    //                 page_folders: {
    //                     main_folder: {
    //                         pages: {index_page, not_found_page, reset_password_page}
    //                     }
    //                 }
    //             },
    //             backend: backend_dev
    //         },
    //         staging_branch: staging_branch::|staging|{
    //             name: staging::{name},
    //             frontend: frontend_staging,
    //             backend: backend_staging
    //         }
    //     },
    //     user_access: user
    // }



    RETURN { 
    app: {
        branches: [
            {
                name: dev_branch::{name},
                frontend:  {
                    page_folders: [
                        {
                            name: main_folder::{name},
                            pages: [index_page, not_found_page, reset_password_page]
                        }
                    ],
                },
                backend: backend_dev
            },
            {
                name: staging_branch::{name},
                frontend: frontend_staging,
                backend: backend_staging
            }
        ],
        name: app::{name},
        description: app::{description},
        id: app::{id},
    }
}
