# Similar Photos

## Requirements

### Grouping

> Please note that you will need to decide the best UX for this new functionality. Existing UX and GUI could be changed
> if needed.

- [ ] New grouping UX and mechanics are introduced. When user selects grouping, then photos view is divided with
  horizontal lines that group photos in one or another way:
    - [ ] Grouping by date (created, modified, etc.)
    - [ ] Grouping by size
    - [ ] Grouping by type
    - [ ] Grouping by quality (requires scanning as it is done by bad photos pipeline)
    - [ ] Grouping by similarity (requires scanning)
- [ ] User is able to group photos **by similarity**. User should be able to select `burst-dense`, `burst-sparse`,
  `same`, `high`, `medium` and `low` similarity. This
  kind of grouping should not take care of photo name, size or file type.
    - [ ] Burst (`burst-dense`) - intelligent way checks following photos and tries to identify frequently taken a
      series of photos. Each following photo that is far from time frequency threshold is treated as "another" photo and
      burst serie stops.
    - [ ] Burst (`burst-sparse`) - similar photos, that follow one after another without any time threshold, are grouped
      together as Sparse Burst. Each following photo that is not so similar to the cluster is treated as "another" and
      sparse burst is broken.
    - [ ] Same - exact photos despite format, size and other attributes, are grouped as same.
    - [ ] High - the whole folder (or scope) is grouped to very similar photos does not matter time taken, size, name,
      file type or any other attribute.
    - [ ] Medium - same as high, but less similar photos.
    - [ ] Low - same as medium, but slightly similar photos.

Ideas that can be used or not (take them as proposal, not strict requirement):

- Perceptual Hashing (pHash) + compared using Hamming distance. If slow, then dHash.
- `image_hasher`
- combine temporal proximity (timestamps) with tight perceptual hash verification

### Collapsing

- [ ] User is able to collapse photos to virtual collapsed folders **by similarity**. The folder has a single photo as a
  headline and other photos behind it indicating that it is a burst kind of catalog. No traditional folder wrapper is
  needed.

### Sorting

- [ ] User is able to sort photos **by similarity**
- [ ] User is able to sort photos **by quality** that is already calculated by bad photos pipeline

## Other Fixes

- [ ] **Virtual folder fix**

1. I open `home > rimvydas > Photos > MobileBackup > iPhone`
2. I press scan on `2014`
3. When scan completed I must be forwarded to `home > rimvydas > Photos > MobileBackup > iPhone > 2014 > [bad photos]`
   virtual folder. Now I'm forwarded to `Collections > Bad photos` - this is bad. Virtual folder must contain the whole
   path so user can easily to step back.

- [ ] **Photos cache improvement**

We already have cache for image thumbnails. We should also cache vectors, tags and/or indexes to quickly re-determine
photo similarity and quality.

- [ ] **Toolbar.tsx improvements**
    - [ ] At the end of path there's a folder that transforms all the visual path to input text field with a written
      path. This allows user to copy the current path and paste existing. Thin about the icon of that button: maybe it
      can be switched with edit mode button icon.
    - [ ] When path is in edit mode, I can paste any other path and I can navigate immediately to that path. Appropriate
      errors should be shown and path should be reverted if bad. In general, this is more like a Microsoft Windows
      behavior.

- [ ] Folder on hover fix: now when folder is on hover, hover looks strange and ugly - improve GUI in that place.

- [ ] **Moder view**
    - [ ] It should be possible to select "Standard" and "Modern" looks. Moder view stops drawing standard folder like
      images that wrap 4 photos, but instead of it, it draws a modern looking box that has thin bezels and photos tries
      taking as much space as possible.
    - [ ] Modern view in general drastically reduces padding and margin of photos views.
    - [ ] Modern view is applicable only to photos view and folders view.