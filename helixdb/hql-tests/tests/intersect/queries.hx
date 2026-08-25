QUERY CreateArticle(title: String) =>
    article <- AddN<Article>({title: title})
    RETURN article

QUERY CreateTag(name: String) =>
    tag <- AddN<Tag>({name: name})
    RETURN tag

QUERY TagArticle(article_id: ID, tag_id: ID) =>
    edge <- AddE<HasTag>::From(article_id)::To(tag_id)
    RETURN edge

// Find articles that share ALL the given tags (intersection)
QUERY ArticlesByAllTags(tag_names: [String]) =>
    articles <- N<Tag>::WHERE(_::{name}::IS_IN(tag_names))::INTERSECT(_::In<HasTag>)
    RETURN articles

QUERY ArticlesByAllTagsTitle(tag_names: [String],name: String) =>
    articles <- N<Tag>::WHERE(_::{name}::IS_IN(tag_names))::INTERSECT(_::In<HasTag>)::WHERE(_::{title}::EQ(name))
    RETURN articles
