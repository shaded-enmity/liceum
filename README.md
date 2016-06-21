[![Travis branch](https://travis-ci.org/shaded-enmity/liceum.svg?branch=master)](https://travis-ci.org/shaded-enmity/liceum)
[![Clippy Linting Result](https://clippy.bashy.io/github/shaded-enmity/liceum/master/badge.svg)](https://clippy.bashy.io/github/shaded-enmity/liceum/master/log)

# Liceum
Advanced license detector written in Rust

### Usage
First we need to generate the datasets from a directory:
```
$ liceum -g /path/to/licenses
```
Output is placed in `$PWD/cache` and consists of `ngrams.json` and `hashes.ssdeep` files.

```
$ liceum /some/project
{
   "/some/project/LICENSE": {
      "mit"
   }
}
```

### Prerequisites

```ssdeep file```

### Theory

The code uses two independent search paths to produce precise data with close to zero chances for false positives. 

#### Ssdeep

ssdeep is a fuzzy hashing mechanism capable of producing similarity index between two files. Files with index greater than 75 (out of 100) are considered during the license recognition.

#### Unique ngrams
Ngrams are extracted from the license corpuses and are crosschecked for uniqueness by constructing a graph mapping corpus nodes to ngram nodes.

```
                            +------+                             +------+       +------+
                            |File A|                             |File B|       |File C|
                            +---+--+                             +--+---+       +---+--+
                                |                                   |               |
                                |                                   |               |
          +---------------------------------------------+           |               |
          |                     |                       |           |               |
          |                     |                       |           |               |
          |                     |                       |           |               |
          |                     |                       |           |               |
          |                     |                       |           |               |
          |                     |                       |           |               |
          |                     |                       |           |               |
          |                     |                       +-----------+>--------------+
          |                     |                       |                           |
          |                     |                       |                           |
          |                     |                       |                           |
          |                     |                       |                           |
          |                     |                       |                           |
          |                     |                       |                           |
+---------v---------+   +-------v--------+   +----------v---------+   +-------------v-------------+
|[This, is, example]|   |is, example, of]|   |[example, of, ngram]|   |[of, ngram, categorization]|
+-------------------+   +----------------+   +--------------------+   +---------------------------+

```

We start by iterating over each ngram and checking the number of edges from that ngram to corpus nodes. If the number is one, that means that that particular ngram is unique to the corpus (at that particular level, but more on that shortly), if the number is greater than one then we do an edge cleanup. Edge cleanup consists of removing edges to already finished corpuses. Corpus is considered finished when `N` unique ngrams were found for that corpus. At the end of each iteration we increase the level value to distinguish between corpuses consisting solely of unique ngrams (1st order) or "redeemed unique" ngrams (nth order, n > 1). "Redeemed unique" means that these ngrams were selected because an edge from that ngram to some now finished corpus was removed during previous iteration.

Let's assume that we're ok with a single ngram per corpus so we start iterating through the ngrams:
```
1: (This, is, example) -> File A
```
First ngram is assigned to `File A` and since there's no other ngram we proceed to next iteration, after the edge cleanup we get:

```
                                                                 +------+       +------+
                                                                 |File B|       |File C|
                                                                 +--+---+       +---+--+
                                                                    |               |
                                                                    |               |
                                                                    |               |
                                                                    |               |
                                                                    |               |
                                                                    |               |
                                                                    |               |
                                                                    |               |
                                                                    |               |
                                                                    |               |
                                                        +-----------+>--------------+
                                                        |                           |
                                                        |                           |
                                                        |                           |
                                                        |                           |
                                                        |                           |
                                                        |                           |
                                             +----------v---------+   +-------------v-------------+
                                             |[example, of, ngram]|   |[of, ngram, categorization]|
                                             +--------------------+   +---------------------------+
```
Third ngram has a single edge to `File B` now:
```
2: (example, of, ngram) -> File B
```

The next iteration the graph looks like this after edge cleanup:
```
          +------+
          |File C|
          +---+--+
              |
              |
              |
              |
              |
              |
              |
              |
              |
              |
              |
              |
              |
              |
              |
              |
              |
+-------------v-------------+
|[of, ngram, categorization]|
+---------------------------+
```
Which gives us:
```
3: (example, of, ngram) -> File C
```

The level information is finally used by the search algorithm to pick the best candidate (lowest level).

### License

GPL-3.0
