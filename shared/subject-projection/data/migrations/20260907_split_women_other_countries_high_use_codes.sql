-- The HQ1451-HQ1870.7 country schedule is generated through Table H9 rather
-- than published as contiguous regional spans. Split only the independently
-- verified, high-use country general-work numbers; keep the parent range for
-- all other countries and for broad source values.

INSERT INTO concept(concept_id,preferred_label) VALUES
  (53448,'Women in Great Britain'),
  (53449,'Women in Germany'),
  (53450,'Women in India'),
  (53451,'Women in Japan'),
  (53452,'Women in China');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (53448,52434,0),(53449,52434,0),(53450,52434,0),
  (53451,52434,0),(53452,52434,0);

INSERT INTO lcc_selector(concept_id,selector) VALUES
  (53448,'HQ1593'),
  (53449,'HQ1623'),
  (53450,'HQ1742'),
  (53451,'HQ1762'),
  (53452,'HQ1767');
