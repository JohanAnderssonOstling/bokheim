"""Rank visually reviewed covers by their complete design.

The three design ratings are curator judgments, not measurements inferred from
pixels or publisher names. No preference is awarded for having an illustration.
Image eligibility (identity, language, scope, scan defects) is established before
ranking. A clean image without a whole-design review is not a preferred design.
"""

import math

TARGET_RATIO = 1.5
DESIGN_FIELDS = ('composition', 'typography', 'readability')
CORE_FIELDS = DESIGN_FIELDS
RATING_LABELS = {0: 'unusable', 1: 'poor', 2: 'usable', 3: 'good', 4: 'excellent'}


def assessment(ratings, note):
    # Older review files included a fourth artwork-context rating. Ignore it:
    # it neither belongs in a current assessment nor affects ranking.
    if len(ratings) == 4:
        ratings = ratings[:3]
    if len(ratings) != len(DESIGN_FIELDS):
        raise ValueError('Three whole-design ratings are required')
    if any(type(value) is not int or value not in RATING_LABELS for value in ratings):
        raise ValueError('Design ratings must be integers from 0 to 4')
    if not note.strip():
        raise ValueError('Record the visual reason for the assessment')
    return {**dict(zip(DESIGN_FIELDS, ratings)), 'note': note, 'method': 'manual whole-cover review'}


def design_band(review):
    if review is None:
        return 'unreviewed'
    values = [review[field] for field in DESIGN_FIELDS]
    assessment(values, review['note'])  # Validate persisted reviews too.
    floor = min(review[field] for field in CORE_FIELDS)
    if floor < 2:
        return 'weak'
    if floor < 3:
        return 'fallback'
    return 'strong'


def aspect_distance(size):
    width, height = size
    if width <= 0 or height <= 0:
        raise ValueError('Cover dimensions must be positive')
    return abs(math.log((height / width) / TARGET_RATIO))


def rank_key(candidate, merged=False):
    """Lower keys rank first; only eligible, reviewed covers can be preferred.

    Correct work scope and an actual cover come before page scans or unrelated
    product imagery. Within actual covers, the weakest of composition,
    typography and readability leads. For equal design ratings, prefer more
    image pixels, then cover proportions. Metadata confidence and edition
    language are not design ratings.
    """
    review = candidate.get('design_review')
    design_band(review)  # Validate a persisted assessment before using it.
    eligible = candidate.get('eligible', False)
    kind = candidate.get('image_kind', 'cover')
    fallback = {'page_scan': 1, 'artifact': 2, 'unavailable': 3}.get(kind, 0)
    core = [review[field] for field in CORE_FIELDS] if review else [0, 0, 0]
    width, height = candidate['size']
    return (
        not eligible,
        bool(merged and candidate.get('single_volume', False)),
        fallback,
        review is None,
        not candidate.get('user_preferred', False),
        -min(core),
        -sum(core),
        -(width * height),
        aspect_distance(candidate['size']),
    )
