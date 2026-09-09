module Main where

-- | A simple factorial function
factorial :: Integer -> Integer
factorial 0 = 1
factorial n = n * factorial (n - 1)

-- | A custom data type
data Point = Point
  { x :: Double
  , y :: Double
  } deriving (Show, Eq)

-- | Calculate distance between two points
distance :: Point -> Point -> Double
distance p1 p2 = sqrt (dx * dx + dy * dy)
  where
    dx = x p1 - x p2
    dy = y p1 - y p2

main :: IO ()
main = do
  putStrLn "Hello, Haskell!"
  print $ factorial 5
  let p1 = Point 0.0 0.0
      p2 = Point 3.0 4.0
  print $ distance p1 p2
